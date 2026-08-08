use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_bundle::wrangler::WranglerConfig;
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
        BuildPlatform::Android => manifest.target == Target::AndroidArm64,
        BuildPlatform::Macos => {
            matches!(manifest.target, Target::MacosArm64 | Target::MacosX64)
        }
        BuildPlatform::Ios => manifest.target == Target::IosArm64,
        BuildPlatform::IosSimulator => matches!(
            manifest.target,
            Target::IosSimulatorArm64 | Target::IosSimulatorX64
        ),
        BuildPlatform::Windows => manifest.target == Target::WindowsX64,
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
        (package_manager("pnpm"), &["run", "build"])
    } else if project.join("yarn.lock").is_file() {
        (package_manager("yarn"), &["build"])
    } else {
        (package_manager("npm"), &["run", "build"])
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

pub(crate) fn package_manager(name: &'static str) -> &'static str {
    if cfg!(windows) {
        match name {
            "npm" => "npm.cmd",
            "pnpm" => "pnpm.cmd",
            "yarn" => "yarn.cmd",
            _ => name,
        }
    } else {
        name
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
        .context("package.json name is required")?;
    if name.is_empty() {
        bail!("package.json name is required");
    }
    if !appd_bundle::is_valid_app_name(name) {
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
    for entry in WalkDir::new(from).follow_links(true) {
        let entry = entry.map_err(std::io::Error::other)?;
        let relative = entry.path().strip_prefix(from)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = to.join(relative);
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            fs::create_dir_all(destination)?;
        } else if metadata.is_file() {
            copy_file(entry.path(), destination)?;
        } else {
            bail!("unsupported file in {}", entry.path().display());
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn copy_dir_contents_preserving_symlinks(from: &Path, to: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;

    for entry in WalkDir::new(from).follow_links(false) {
        let entry = entry.map_err(std::io::Error::other)?;
        let relative = entry.path().strip_prefix(from)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = to.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(destination)?;
        } else if entry.file_type().is_symlink() {
            let parent = destination
                .parent()
                .context("symlink destination must have a parent")?;
            fs::create_dir_all(parent)?;
            symlink(fs::read_link(entry.path())?, destination)?;
        } else if entry.file_type().is_file() {
            copy_file(entry.path(), destination)?;
        } else {
            bail!("unsupported file in {}", entry.path().display());
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn copy_dir_contents_preserving_symlinks(from: &Path, to: &Path) -> Result<()> {
    copy_dir_contents(from, to)
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

#[cfg(test)]
mod tests {
    use super::{copy_dir_contents, copy_dir_contents_preserving_symlinks, package_manager};

    #[test]
    fn selects_platform_package_manager_commands() {
        let suffix = if cfg!(windows) { ".cmd" } else { "" };
        for name in ["npm", "pnpm", "yarn"] {
            assert_eq!(package_manager(name), format!("{name}{suffix}"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn copies_link_targets_as_regular_files() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir()?;
        let source = temporary.path().join("source");
        let destination = temporary.path().join("destination");
        std::fs::create_dir_all(&source)?;
        std::fs::write(source.join("target"), "content")?;
        symlink("target", source.join("link"))?;

        copy_dir_contents(&source, &destination)?;

        assert_eq!(
            std::fs::read_to_string(destination.join("link"))?,
            "content"
        );
        assert!(
            !std::fs::symlink_metadata(destination.join("link"))?
                .file_type()
                .is_symlink()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn preserves_framework_links() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;
        use std::path::Path;

        let temporary = tempfile::tempdir()?;
        let source = temporary.path().join("source");
        let destination = temporary.path().join("destination");
        std::fs::create_dir_all(source.join("Versions/A"))?;
        std::fs::write(source.join("Versions/A/BareKit"), "runtime")?;
        symlink("A", source.join("Versions/Current"))?;
        symlink("Versions/Current/BareKit", source.join("BareKit"))?;

        copy_dir_contents_preserving_symlinks(&source, &destination)?;

        assert_eq!(
            std::fs::read_link(destination.join("BareKit"))?,
            Path::new("Versions/Current/BareKit")
        );
        assert_eq!(
            std::fs::read_link(destination.join("Versions/Current"))?,
            Path::new("A")
        );
        Ok(())
    }
}
