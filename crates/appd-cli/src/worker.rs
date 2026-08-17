use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_bundle::AppLayout;
use appd_bundle::assets::write_manifest;
use appd_bundle::compress_worker_bundle;
use appd_bundle::environment::{WorkerEnvironment, write as write_environment};
use appd_bundle::wrangler::WranglerConfig;
use appd_quickjs::compile_worker;
use appd_target_pack::{ArtifactKind, TargetPackManifest};
use walkdir::WalkDir;

use super::support::{artifact_path, copy_dir_contents};

pub(crate) fn prepare_quickjs_app(
    app_dir: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    wrangler: &WranglerConfig,
) -> Result<()> {
    let worker_root = wrangler
        .main
        .parent()
        .context("worker main must have a parent directory")?;
    reject_wasm_files(worker_root)?;
    let layout = AppLayout::new(app_dir);
    write_environment(
        &layout,
        &WorkerEnvironment {
            vars: wrangler.vars.clone(),
        },
    )?;
    if let Some(assets) = &wrangler.assets {
        reject_wasm_files(&assets.directory)?;
        copy_dir_contents(&assets.directory, &layout.assets())?;
        write_manifest(&layout, assets)?;
    }
    compile_worker_bundle(&layout, &wrangler.main, pack_root, manifest)
}

fn compile_worker_bundle(
    layout: &AppLayout,
    worker: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
) -> Result<()> {
    let runtime = artifact_path(
        pack_root,
        manifest,
        &ArtifactKind::RuntimeJavaScriptDirectory,
    )?;
    let compiler = artifact_path(pack_root, manifest, &ArtifactKind::EsbuildExecutable)?;
    let source = layout.root().join("worker.source.mjs");
    let status = Command::new(node_path(&compiler))
        .args([
            "--bundle",
            "--format=esm",
            "--platform=neutral",
            "--target=es2022",
        ])
        .arg(format!(
            "--alias:node:events={}",
            node_path(&runtime.join("events.mjs")).display()
        ))
        .arg(format!(
            "--alias:node:stream={}",
            node_path(&runtime.join("stream.mjs")).display()
        ))
        .arg(format!(
            "--alias:node:fs={}",
            node_path(&runtime.join("fs.mjs")).display()
        ))
        .arg(format!(
            "--alias:node:fs/promises={}",
            node_path(&runtime.join("fs.mjs")).display()
        ))
        .arg(format!(
            "--alias:cloudflare:workers={}",
            node_path(&runtime.join("cloudflare-workers.mjs")).display()
        ))
        .arg(format!("--outfile={}", node_path(&source).display()))
        .arg(node_path(worker))
        .status()
        .context("failed to bundle the Worker with esbuild")?;
    if !status.success() {
        bail!("Worker bundling failed with status {status}");
    }
    let bytecode = compile_worker(&fs::read(&source)?)?;
    fs::write(layout.worker_bundle(), compress_worker_bundle(&bytecode)?)?;
    fs::remove_file(source)?;
    Ok(())
}

fn node_path(path: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(value) = value.strip_prefix(r"\\?\")
            && value.as_bytes().get(1) == Some(&b':')
        {
            return value.into();
        }
    }
    path.into()
}

fn reject_wasm_files(root: &Path) -> Result<()> {
    for entry in WalkDir::new(root) {
        let entry = entry.map_err(std::io::Error::other)?;
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("wasm"))
        {
            bail!(
                "WebAssembly files are not supported: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}
