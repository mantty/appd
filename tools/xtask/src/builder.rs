use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use target_pack_format::{
    Artifact, ESBUILD_DIRECTORY, RUNTIME_DIRECTORY, RUNTIME_JAVASCRIPT_DIRECTORY, Target,
    TargetPackManifest, TargetPackVersion, write_manifest,
};

use crate::layout::WorkspaceLayout;
use crate::support::copy_dir_contents;

pub(crate) fn build_source_target_pack(target: Target) -> Result<PathBuf> {
    let workspace = WorkspaceLayout::from_source()
        .context("source workspace is unavailable; run this command from an appd checkout")?;
    build_source_target_pack_at(&workspace, target)
}

fn build_source_target_pack_at(workspace: &WorkspaceLayout, target: Target) -> Result<PathBuf> {
    let pack_dir = workspace.target_pack(target);
    let recipe_output = workspace.recipe_output(target);
    reset_dir(&pack_dir)?;
    reset_dir(&recipe_output)?;

    run_platform_recipe(workspace, target, &recipe_output)?;
    copy_dir_contents(&recipe_output, &pack_dir).context("copy platform target-pack artifacts")?;
    fs::remove_dir_all(&recipe_output)?;

    deploy_runtime(workspace, &pack_dir)?;
    let artifacts = target.artifacts();
    validate_artifacts(&pack_dir, &artifacts)?;

    let manifest = TargetPackManifest {
        schema_version: TargetPackVersion::CURRENT,
        appd_version: env!("CARGO_PKG_VERSION").to_owned(),
        target,
        artifacts,
        required_tools: target
            .required_tools()
            .iter()
            .map(|tool| (*tool).to_owned())
            .collect(),
    };
    let manifest_path = workspace.manifest(target);
    write_manifest(&manifest_path, &manifest)?;
    Ok(manifest_path)
}

fn run_platform_recipe(workspace: &WorkspaceLayout, target: Target, output: &Path) -> Result<()> {
    let recipe = workspace.platform_recipe(target);
    if !recipe.is_file() {
        bail!(
            "platform target-pack recipe is missing: {}",
            recipe.display()
        );
    }

    let mut command = if recipe
        .extension()
        .is_some_and(|extension| extension == "ps1")
    {
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        command.arg(&recipe);
        command
    } else {
        let mut command = Command::new("bash");
        command.arg(&recipe);
        command
    };
    let target_name = target.to_string();
    let status = command
        .args(["build", &target_name, target.rust_target()])
        .arg(output)
        .current_dir(workspace.root())
        .status()
        .with_context(|| format!("failed to run {}", recipe.display()))?;
    if status.success() {
        Ok(())
    } else {
        bail!(
            "platform target-pack recipe failed with status {status}: {}",
            recipe.display()
        )
    }
}

fn deploy_runtime(workspace: &WorkspaceLayout, pack_root: &Path) -> Result<()> {
    let runtime = pack_root.join(RUNTIME_DIRECTORY);
    reset_dir(&runtime)?;
    copy_dir_contents(&workspace.esbuild(), &pack_root.join(ESBUILD_DIRECTORY))?;
    copy_dir_contents(
        &workspace.runtime_javascript(),
        &pack_root.join(RUNTIME_JAVASCRIPT_DIRECTORY),
    )
}

fn validate_artifacts(root: &Path, artifacts: &[Artifact]) -> Result<()> {
    for artifact in artifacts {
        let path = root.join(&artifact.path);
        if !path.exists() {
            bail!("target-pack artifact was not produced: {}", path.display());
        }
    }
    Ok(())
}

fn reset_dir(path: &Path) -> Result<()> {
    if path.is_symlink() || path.is_file() {
        fs::remove_file(path)?;
    } else if path.is_dir() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::validate_artifacts;
    use target_pack_format::{Artifact, ArtifactKind};

    #[test]
    fn rejects_missing_pack_artifacts() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let result = validate_artifacts(
            directory.path(),
            &[Artifact {
                kind: ArtifactKind::RuntimeJavaScriptDirectory,
                path: "runtime".to_owned(),
            }],
        );

        assert!(result.is_err_and(|error| {
            error
                .to_string()
                .contains("target-pack artifact was not produced")
        }));
        Ok(())
    }

    #[test]
    fn accepts_files_and_directories_from_the_recipe() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        fs::create_dir(directory.path().join("runtime"))?;
        fs::write(directory.path().join("runtime/file"), "runtime")?;

        validate_artifacts(
            directory.path(),
            &[Artifact {
                kind: ArtifactKind::RuntimeJavaScriptDirectory,
                path: "runtime".to_owned(),
            }],
        )?;
        Ok(())
    }
}
