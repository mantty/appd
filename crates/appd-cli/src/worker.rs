use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_bundle::AppLayout;
use appd_bundle::assets::write_manifest;
use appd_bundle::environment::{WorkerEnvironment, write as write_environment};
use appd_bundle::wrangler::WranglerConfig;
use appd_target_pack::{ArtifactKind, Target, TargetPackManifest};
use walkdir::WalkDir;

use super::support::{artifact_path, copy_dir_contents};

pub(crate) fn prepare_bare_app(
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
    pack_worker(&layout, &wrangler.main, pack_root, manifest)
}

fn pack_worker(
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
    let packer = artifact_path(pack_root, manifest, &ArtifactKind::BarePackExecutable)?;
    let compiler = artifact_path(pack_root, manifest, &ArtifactKind::EsbuildExecutable)?;
    let builtins = artifact_path(pack_root, manifest, &ArtifactKind::BareRuntimeDirectory)?
        .join("builtins.json");
    let app_dir = layout.root();
    let entry = app_dir.join("appd-worklet.cjs");
    let modules = runtime
        .parent()
        .context("runtime JavaScript directory must have a parent directory")?
        .join("node_modules");
    let modules_link = app_dir.join("node_modules");
    copy_dir_contents(&modules, &modules_link)?;

    let result = (|| {
        compile_worker(&entry, &runtime, &compiler, worker)?;
        let output = layout.worker_bundle();
        let mut command = Command::new(node_executable());
        command
            .arg(node_path(&packer))
            .args(["--builtins"])
            .arg(node_path(&builtins))
            .args(["--host", bare_host(manifest.target), "--base", "/"])
            .arg("--out")
            .arg(node_path(&output))
            .arg(node_path(&entry));
        let status = command
            .status()
            .context("failed to run the Bare bundle packer")?;
        if !status.success() {
            bail!("Bare bundle packing failed with status {status}");
        }
        Ok(())
    })();

    let _ = fs::remove_file(entry);
    let _ = fs::remove_dir_all(modules_link);
    result
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

fn node_executable() -> &'static str {
    if cfg!(windows) { "node.exe" } else { "node" }
}

fn compile_worker(output: &Path, runtime: &Path, compiler: &Path, worker: &Path) -> Result<()> {
    let status = Command::new(node_executable())
        .arg(node_path(&runtime.join("pack-worker.js")))
        .arg("--compiler")
        .arg(node_path(compiler))
        .arg("--worker")
        .arg(node_path(worker))
        .arg("--output")
        .arg(node_path(output))
        .status()
        .context("failed to run the runtime worker packer")?;
    if status.success() {
        Ok(())
    } else {
        bail!("runtime worker packing failed with status {status}")
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
        Target::WindowsX64 => "win32-x64",
        Target::MacosArm64 => "darwin-arm64",
        Target::MacosX64 => "darwin-x64",
        Target::IosArm64 => "ios-arm64",
        Target::IosSimulatorArm64 => "ios-arm64-simulator",
        Target::IosSimulatorX64 => "ios-x64-simulator",
    }
}

#[cfg(test)]
mod tests {
    use super::node_executable;

    #[cfg(not(windows))]
    #[test]
    fn uses_node_on_unix() {
        assert_eq!(node_executable(), "node");
    }

    #[cfg(windows)]
    #[test]
    fn uses_node_exe_on_windows() {
        assert_eq!(node_executable(), "node.exe");
    }

    #[cfg(windows)]
    #[test]
    fn strips_extended_prefix_from_drive_paths_only() {
        use std::path::{Path, PathBuf};

        assert_eq!(
            super::node_path(Path::new(r"\\?\C:\app\worker.cjs")),
            PathBuf::from(r"C:\app\worker.cjs")
        );
        assert_eq!(
            super::node_path(Path::new(r"\\?\UNC\server\share\worker.cjs")),
            PathBuf::from(r"\\?\UNC\server\share\worker.cjs")
        );
    }
}
