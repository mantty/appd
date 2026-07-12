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
const RUNTIME_BINARY: &str = "appd-runtime";

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
    build_source_target_pack(&workspace, target)
}

fn default_target(platform: BuildPlatform) -> Result<Target> {
    match platform {
        BuildPlatform::Ios => Ok(Target::IosArm64),
        BuildPlatform::Macos if cfg!(target_arch = "aarch64") => Ok(Target::MacosArm64),
        BuildPlatform::Macos => bail!("macOS builds currently require an Apple Silicon host"),
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

fn build_source_target_pack(workspace: &Path, target: Target) -> Result<PathBuf> {
    let rust_target = rust_runtime_target(target);
    build_bare_sdk(workspace, target)?;
    build_runtime_tools(workspace)?;
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
            "bare-runtime",
            "--target",
            rust_target,
        ])
        .current_dir(workspace)
        .status()
        .context("failed to run cargo build for appd-runtime")?;
    if !status.success() {
        bail!("appd-runtime build failed with status {status}");
    }

    let runtime_binary = RUNTIME_BINARY;
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
    let tools_dir = pack_dir.join("tools");
    deploy_runtime(workspace, &tools_dir.join("runtime"))?;
    install_bare_tls(workspace, &tools_dir.join("runtime"), target)?;
    deploy_packer(workspace, &tools_dir.join("packer"))?;

    let manifest = TargetPackManifest {
        schema_version: TargetPackVersion::CURRENT,
        appd_version: env!("CARGO_PKG_VERSION").to_owned(),
        target,
        artifacts: vec![
            Artifact {
                kind: ArtifactKind::RuntimeExecutable,
                path: format!("bin/{runtime_binary}"),
                sha256: None,
            },
            Artifact {
                kind: ArtifactKind::RuntimeJavaScriptDirectory,
                path: "tools/runtime/runtime-js".to_owned(),
                sha256: None,
            },
            Artifact {
                kind: ArtifactKind::BarePackExecutable,
                path: "tools/packer/node_modules/bare-pack/bin.js".to_owned(),
                sha256: None,
            },
            Artifact {
                kind: ArtifactKind::EsbuildExecutable,
                path: "tools/packer/node_modules/esbuild/bin/esbuild".to_owned(),
                sha256: None,
            },
        ],
        required_tools: vec!["node".to_owned()],
    };
    write_manifest(pack_dir.join("target-pack.json"), &manifest)?;

    Ok(pack_dir.join("target-pack.json"))
}

fn build_bare_sdk(workspace: &Path, target: Target) -> Result<()> {
    let sdk_target = match target {
        Target::MacosArm64 => "macos-arm64",
        Target::IosArm64 => "ios-arm64",
    };
    let output = workspace.join("target/bare/sdk").join(sdk_target);
    if env::var_os("APPD_BARE_SDK_DIR").is_some() && output.join("sdk-manifest.json").is_file() {
        return Ok(());
    }
    let status = Command::new("python3")
        .arg("bare/scripts/build-sdk.py")
        .args(["--target", sdk_target, "--output"])
        .arg(&output)
        .current_dir(workspace)
        .status()
        .context("failed to build the Bare SDK")?;
    if status.success() {
        Ok(())
    } else {
        bail!("Bare SDK build failed with status {status}")
    }
}

fn build_runtime_tools(workspace: &Path) -> Result<()> {
    let script = "build:runtime";
    let status = Command::new("pnpm")
        .args(["run", script])
        .current_dir(workspace)
        .status()
        .with_context(|| format!("failed to run pnpm {script}"))?;
    if !status.success() {
        bail!("pnpm {script} failed with status {status}");
    }
    Ok(())
}

fn install_bare_tls(workspace: &Path, runtime: &Path, target: Target) -> Result<()> {
    let sdk_target = match target {
        Target::MacosArm64 => "macos-arm64",
        Target::IosArm64 => "ios-arm64",
    };
    let host = match target {
        Target::MacosArm64 => "darwin-arm64",
        Target::IosArm64 => "ios-arm64",
    };
    let source = workspace
        .join("target/bare/sdk")
        .join(sdk_target)
        .join("bare-tls.bare");
    let destination = runtime
        .join("node_modules/bare-tls/prebuilds")
        .join(host)
        .join("bare-tls.bare");
    copy_file(&source, destination)
}

fn deploy_runtime(workspace: &Path, output: &Path) -> Result<()> {
    let status = Command::new("pnpm")
        .args([
            "--config.node-linker=isolated",
            "--filter",
            "appd",
            "deploy",
            "--prod",
        ])
        .arg(output)
        .current_dir(workspace)
        .status()
        .context("failed to deploy Bare runtime modules")?;
    if !status.success() {
        bail!("Bare runtime module deployment failed with status {status}")
    }
    copy_directory(
        &workspace.join("target/runtime-js"),
        &output.join("runtime-js"),
    )
}

fn deploy_packer(workspace: &Path, output: &Path) -> Result<()> {
    let status = Command::new("pnpm")
        .args([
            "--config.node-linker=isolated",
            "--filter",
            "@appd/bare-pack-tool",
            "deploy",
            "--prod",
        ])
        .arg(output)
        .current_dir(workspace)
        .status()
        .context("failed to deploy Bare bundle packer")?;
    if status.success() {
        Ok(())
    } else {
        bail!("Bare bundle packer deployment failed with status {status}")
    }
}

fn rust_runtime_target(target: Target) -> &'static str {
    match target {
        Target::MacosArm64 => "aarch64-apple-darwin",
        Target::IosArm64 => "aarch64-apple-ios",
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

fn copy_directory(from: &Path, to: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(from) {
        let entry = entry.map_err(std::io::Error::other)?;
        let relative = entry.path().strip_prefix(from)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = to.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(destination)?;
        } else if entry.file_type().is_file() {
            copy_file(entry.path(), destination)?;
        }
    }
    Ok(())
}
