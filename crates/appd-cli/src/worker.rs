use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_runtime::assets::write_manifest;
use appd_runtime::host::{ASSET_DIRECTORY, WORKER_BUNDLE_FILE};
use appd_runtime::wrangler_config::WranglerConfig;
use appd_target_pack::{ArtifactKind, Target, TargetPackManifest};
use walkdir::WalkDir;

use super::support::{artifact_path, copy_dir_contents};

pub(crate) fn prepare_bare_app(
    app_dir: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    wrangler: &WranglerConfig,
) -> Result<()> {
    prepare_bare_app_contents(app_dir, pack_root, manifest, wrangler)
}

pub(crate) fn prepare_android_bare_app(
    app_dir: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    wrangler: &WranglerConfig,
) -> Result<()> {
    prepare_bare_app_contents(app_dir, pack_root, manifest, wrangler)
}

fn prepare_bare_app_contents(
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
    if let Some(assets) = &wrangler.assets {
        reject_wasm_files(&assets.directory)?;
        copy_dir_contents(&assets.directory, &app_dir.join(ASSET_DIRECTORY))?;
        write_manifest(app_dir, assets)?;
    }
    let runtime = artifact_path(
        pack_root,
        manifest,
        &ArtifactKind::RuntimeJavaScriptDirectory,
    )?;
    let packer = artifact_path(pack_root, manifest, &ArtifactKind::BarePackExecutable)?;
    let compiler = artifact_path(pack_root, manifest, &ArtifactKind::EsbuildExecutable)?;
    pack_worker(
        app_dir,
        &runtime,
        &packer,
        &compiler,
        &wrangler.main,
        manifest.target,
    )
}

fn pack_worker(
    app_dir: &Path,
    runtime: &Path,
    packer: &Path,
    compiler: &Path,
    worker: &Path,
    target: Target,
) -> Result<()> {
    let entry = app_dir.join("appd-worklet.cjs");
    let modules = runtime
        .parent()
        .context("runtime JavaScript directory must have a parent directory")?
        .join("node_modules");
    let modules_link = app_dir.join("node_modules");
    let builtins = app_dir.join(".appd-builtins.json");
    symlink(&modules, &modules_link)?;
    write_builtins(&modules, &builtins)?;

    let result = (|| {
        compile_commonjs(&entry, runtime, compiler, worker)?;
        let output = app_dir.join(WORKER_BUNDLE_FILE);
        let status = Command::new("node")
            .arg(packer)
            .args(["--builtins"])
            .arg(&builtins)
            .args(["--host", bare_host(target), "--base", "/"])
            .arg("--out")
            .arg(&output)
            .arg(&entry)
            .status()
            .context("failed to run the Bare bundle packer")?;
        if !status.success() {
            bail!("Bare bundle packing failed with status {status}");
        }
        Ok(())
    })();

    let _ = fs::remove_file(entry);
    let _ = fs::remove_file(modules_link);
    let _ = fs::remove_file(builtins);
    result
}

fn write_builtins(modules: &Path, output: &Path) -> Result<()> {
    let mut addons = BTreeSet::new();

    for entry in WalkDir::new(modules).follow_links(true) {
        let entry = entry.map_err(std::io::Error::other)?;
        if entry.file_name() != "package.json" {
            continue;
        }
        let metadata: serde_json::Value = serde_json::from_slice(&fs::read(entry.path())?)?;
        if metadata["addon"].as_bool() != Some(true) {
            continue;
        }
        let name = metadata["name"]
            .as_str()
            .context("Bare addon package has no name")?
            .to_owned();
        addons.insert(name);
    }

    let builtins: Vec<_> = addons
        .into_iter()
        .map(|addon| serde_json::json!({ "addon": addon }))
        .collect();
    fs::write(output, serde_json::to_vec(&builtins)?)?;
    Ok(())
}

fn compile_commonjs(output: &Path, runtime: &Path, compiler: &Path, worker: &Path) -> Result<()> {
    let aliases = [
        format!("--alias:appd-worker={}", worker.display()),
        format!(
            "--alias:cloudflare:workers={}",
            runtime.join("cloudflare.js").display()
        ),
        "--alias:node:events=bare-events".to_owned(),
        "--alias:node:stream=bare-stream".to_owned(),
        format!("--outfile={}", output.display()),
    ];
    let status = Command::new(compiler)
        .args([
            "--bundle",
            "--format=cjs",
            "--platform=neutral",
            "--target=safari15",
            "--packages=external",
        ])
        .args(aliases)
        .arg(runtime.join("entry.js"))
        .status()
        .context("failed to run the CommonJS compiler")?;
    if status.success() {
        Ok(())
    } else {
        bail!("CommonJS compilation failed with status {status}")
    }
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

fn bare_host(target: Target) -> &'static str {
    match target {
        Target::AndroidArm64 => "android-arm64",
        Target::MacosArm64 => "darwin-arm64",
        Target::MacosX64 => "darwin-x64",
        Target::IosArm64 => "ios-arm64",
        Target::IosSimulatorArm64 => "ios-arm64-simulator",
        Target::IosSimulatorX64 => "ios-x64-simulator",
    }
}
