use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_target_pack::{
    Artifact, ArtifactKind, Target, TargetPackManifest, TargetPackVersion, write_manifest,
};

use crate::BuildPlatform;

const TARGET_PACK_DIR_ENV: &str = "appd_target_pack_dir";

pub(crate) fn resolve_manifest(
    platform: BuildPlatform,
    explicit_manifest: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(manifest) = explicit_manifest {
        return Ok(manifest.to_path_buf());
    }

    let target = default_target(platform)?;
    if let Some(root) = env::var_os(TARGET_PACK_DIR_ENV) {
        return manifest_from_root(Path::new(&root), target).with_context(|| {
            format!("{TARGET_PACK_DIR_ENV} does not contain a target pack for {target}")
        });
    }

    if let Some(manifest) = bundled_manifest(target) {
        return Ok(manifest);
    }

    let workspace = source_workspace_root()
        .context("no bundled target pack found and source workspace is unavailable")?;
    build_source_target_pack(&workspace, platform, target)
}

fn default_target(platform: BuildPlatform) -> Result<Target> {
    match platform {
        BuildPlatform::Macos if cfg!(target_arch = "aarch64") => Ok(Target::MacosArm64),
        BuildPlatform::Macos if cfg!(target_arch = "x86_64") => Ok(Target::MacosX64),
        BuildPlatform::IosSimulator if cfg!(target_arch = "aarch64") => {
            Ok(Target::IosSimulatorArm64)
        }
        BuildPlatform::IosSimulator if cfg!(target_arch = "x86_64") => Ok(Target::IosSimulatorX64),
        BuildPlatform::Ios => Ok(Target::IosArm64),
        BuildPlatform::Android => Ok(Target::AndroidArm64),
        BuildPlatform::Windows => Ok(Target::WindowsX64),
        BuildPlatform::Linux => Ok(Target::LinuxX64),
        BuildPlatform::Macos | BuildPlatform::IosSimulator => {
            bail!("{platform:?} target packs are unavailable for this host architecture")
        }
    }
}

fn manifest_from_root(root: &Path, target: Target) -> Result<PathBuf> {
    let manifest = root.join(target.to_string()).join("target-pack.json");
    if manifest.is_file() {
        Ok(manifest)
    } else {
        bail!("target-pack manifest not found: {}", manifest.display())
    }
}

fn bundled_manifest(target: Target) -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    for root in [
        exe_dir.join("target-packs"),
        exe_dir.join("../share/appd/target-packs"),
        exe_dir.join("../Resources/target-packs"),
    ] {
        let manifest = root.join(target.to_string()).join("target-pack.json");
        if manifest.is_file() {
            return Some(manifest);
        }
    }
    None
}

fn source_workspace_root() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir.parent()?.parent()?.to_path_buf();
    if workspace.join("Cargo.toml").is_file() && workspace.join("crates/appd-runtime").is_dir() {
        Some(workspace)
    } else {
        None
    }
}

fn build_source_target_pack(
    workspace: &Path,
    platform: BuildPlatform,
    target: Target,
) -> Result<PathBuf> {
    let rust_target = rust_runtime_target(target)?;
    eprintln!("Building appd runtime target pack for {target}...");
    let status = Command::new("cargo")
        .args([
            "build",
            "-p",
            "appd-runtime",
            "--bin",
            "appd-runtime",
            "--release",
            "--features",
            "workerd-ffi",
            "--target",
            rust_target,
        ])
        .current_dir(workspace)
        .status()
        .context("failed to run cargo build for appd-runtime")?;
    if !status.success() {
        bail!("appd-runtime build failed with status {status}");
    }

    let runtime_binary = runtime_binary_name(platform);
    let binary = workspace
        .join("target")
        .join(rust_target)
        .join("release")
        .join(runtime_binary);
    if !binary.is_file() {
        bail!("runtime binary was not produced: {}", binary.display());
    }

    let pack_dir = workspace
        .join("target/appd-target-packs")
        .join(target.to_string());
    reset_dir(&pack_dir)?;
    let bin_dir = pack_dir.join("bin");
    fs::create_dir_all(&bin_dir)?;
    copy_file(&binary, bin_dir.join(runtime_binary))?;

    let manifest = TargetPackManifest {
        schema_version: TargetPackVersion::CURRENT,
        appd_version: env!("CARGO_PKG_VERSION").to_owned(),
        target,
        artifacts: vec![Artifact {
            kind: ArtifactKind::RuntimeExecutable,
            path: format!("bin/{runtime_binary}"),
            sha256: None,
        }],
        required_tools: Vec::new(),
    };
    write_manifest(pack_dir.join("target-pack.json"), &manifest)?;

    Ok(pack_dir.join("target-pack.json"))
}

fn rust_runtime_target(target: Target) -> Result<&'static str> {
    match target {
        Target::MacosArm64 => Ok("aarch64-apple-darwin"),
        Target::IosSimulatorArm64 => Ok("aarch64-apple-ios-sim"),
        Target::IosSimulatorX64 => Ok("x86_64-apple-ios"),
        Target::IosArm64 => Ok("aarch64-apple-ios"),
        _ => bail!("source target-pack builds are not implemented for {target}"),
    }
}

fn runtime_binary_name(platform: BuildPlatform) -> &'static str {
    match platform {
        BuildPlatform::Windows => "appd-runtime.exe",
        _ => "appd-runtime",
    }
}

fn reset_dir(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

fn copy_file(from: &Path, to: impl AsRef<Path>) -> Result<()> {
    let to = to.as_ref();
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to).with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
    Ok(())
}
