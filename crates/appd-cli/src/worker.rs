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

use super::support::{artifact_path, copy_dir_contents, copy_file};

pub(crate) fn prepare_bare_app(
    app_dir: &Path,
    frameworks: &Path,
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
    install_bare_addons(&runtime, frameworks, manifest.target)?;
    pack_worker(
        app_dir,
        &runtime,
        &packer,
        &compiler,
        &wrangler.main,
        manifest.target,
    )
}

fn install_bare_addons(runtime: &Path, frameworks: &Path, target: Target) -> Result<()> {
    let modules = runtime
        .parent()
        .context("runtime JavaScript directory must have a parent directory")?
        .join("node_modules");
    if !modules.is_dir() {
        return Ok(());
    }
    for entry in WalkDir::new(&modules).follow_links(true) {
        let entry = entry.map_err(std::io::Error::other)?;
        if entry.file_name() != "package.json" || !is_bare_addon(entry.path())? {
            continue;
        }
        install_bare_addon(entry.path(), frameworks, target)?;
    }
    Ok(())
}

fn install_bare_addon(manifest: &Path, frameworks: &Path, target: Target) -> Result<()> {
    let package = manifest
        .parent()
        .context("Bare addon manifest must have a parent directory")?;
    let metadata: serde_json::Value = serde_json::from_slice(&fs::read(manifest)?)?;
    let name = metadata["name"]
        .as_str()
        .context("Bare addon package has no name")?;
    let version = metadata["version"]
        .as_str()
        .context("Bare addon package has no version")?;
    let module = addon_name(name);
    let source = package
        .join("prebuilds")
        .join(bare_host(target))
        .join(format!("{module}.bare"));
    if !source.is_file() {
        bail!("Bare addon prebuild is missing: {}", source.display());
    }
    let destination = frameworks
        .join(format!("{module}.{version}.framework"))
        .join(format!("{module}.{version}"));
    copy_file(source, &destination)?;
    let framework = frameworks.join(format!("{module}.{version}.framework"));
    write_bare_addon_plist(&framework, &format!("{module}.{version}"))
}

fn write_bare_addon_plist(framework: &Path, executable: &str) -> Result<()> {
    fs::write(
        framework.join("Info.plist"),
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\n  <key>CFBundleIdentifier</key><string>com.appd.bare.{executable}</string>\n  <key>CFBundleExecutable</key><string>{executable}</string>\n  <key>CFBundlePackageType</key><string>FMWK</string>\n  <key>CFBundleVersion</key><string>1</string>\n</dict></plist>\n"
        ),
    )?;
    Ok(())
}

fn is_bare_addon(manifest: &Path) -> Result<bool> {
    let metadata: serde_json::Value = serde_json::from_slice(&fs::read(manifest)?)?;
    Ok(metadata["addon"].as_bool() == Some(true))
}

fn addon_name(name: &str) -> String {
    name.trim_start_matches('@').replace('/', "__")
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
    symlink(&modules, &modules_link)?;

    let result = (|| {
        compile_commonjs(&entry, runtime, compiler, worker)?;
        let output = app_dir.join(WORKER_BUNDLE_FILE);
        let status = Command::new("node")
            .arg(packer)
            .args(["--linked", "--host", bare_host(target), "--base", "/"])
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
    result
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
        Target::MacosArm64 => "darwin-arm64",
        Target::IosArm64 => "ios-arm64",
    }
}
