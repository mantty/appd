use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_runtime::wrangler_config::WranglerConfig;
use appd_target_pack::{ArtifactKind, Target, TargetPackManifest};
use walkdir::WalkDir;

use crate::BuildPlatform;

pub(crate) fn artifact_path(
    pack_root: &Path,
    manifest: &TargetPackManifest,
    kind: &ArtifactKind,
) -> Result<PathBuf> {
    manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == *kind)
        .map(|artifact| pack_root.join(&artifact.path))
        .with_context(|| format!("target pack missing {kind:?} artifact"))
}

pub(crate) fn validate_target(
    manifest: &TargetPackManifest,
    platform: BuildPlatform,
) -> Result<()> {
    let matches = match platform {
        BuildPlatform::Macos => manifest.target == Target::MacosArm64,
        BuildPlatform::Ios => manifest.target == Target::IosArm64,
    };
    if matches {
        Ok(())
    } else {
        bail!(
            "target pack {} cannot build {}",
            manifest.target,
            platform.display_name()
        );
    }
}

pub(crate) fn run_web_build(project: &Path) -> Result<()> {
    if !project.join("package.json").is_file() {
        bail!("package.json not found in {}", project.display());
    }
    let (program, arguments): (&str, &[&str]) = if project.join("pnpm-lock.yaml").is_file() {
        ("pnpm", &["run", "build"])
    } else if project.join("yarn.lock").is_file() {
        ("yarn", &["build"])
    } else {
        ("npm", &["run", "build"])
    };
    let status = Command::new(program)
        .args(arguments)
        .current_dir(project)
        .status()
        .with_context(|| format!("failed to run {program}"))?;
    if status.success() {
        Ok(())
    } else {
        bail!("{program} build failed with status {status}")
    }
}

pub(crate) fn validate_web_build(config: &WranglerConfig) -> Result<()> {
    if !config.main.is_file() {
        bail!("worker main not found: {}", config.main.display());
    }
    if let Some(assets) = &config.assets
        && !assets.directory.is_dir()
    {
        bail!("assets directory not found: {}", assets.directory.display());
    }
    Ok(())
}

pub(crate) fn read_package_name(project: &Path) -> Result<String> {
    let content = fs::read_to_string(project.join("package.json"))?;
    let json: serde_json::Value = serde_json::from_str(&content)?;
    let name = json
        .get("name")
        .and_then(serde_json::Value::as_str)
        .context("package.json name must be a string")?;
    let is_safe = !name.is_empty()
        && !matches!(name, "." | "..")
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character));
    if !is_safe {
        bail!("package.json name is not a safe app name: {name}");
    }
    Ok(name.to_owned())
}

pub(crate) fn build_dir(project: &Path, platform: BuildPlatform) -> PathBuf {
    project.join("build").join(platform.build_dir_name())
}

pub(crate) fn reset_path(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub(crate) fn copy_file(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to).with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
    Ok(())
}

pub(crate) fn copy_dir_contents(from: &Path, to: &Path) -> Result<()> {
    for entry in WalkDir::new(from) {
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

#[cfg(unix)]
pub(crate) fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}
