use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use appd_bundle::wrangler::WranglerConfig;
use appd_target_pack::{ArtifactKind, TargetPackManifest};
use serde::Serialize;

use super::support::{artifact_path, build_dir, copy_file, reset_path};
use super::worker::prepare_quickjs_app;
use super::{BuildPlatform, BuildSummary};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellConfig<'a> {
    name: &'a str,
    host: String,
}

pub(crate) fn build_windows(
    project: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    wrangler: &WranglerConfig,
) -> Result<BuildSummary> {
    let output = build_dir(project, BuildPlatform::Windows).join(app_name);
    reset_path(&output)?;
    let app = output.join("app");
    fs::create_dir_all(&app)?;

    prepare_quickjs_app(&app, pack_root, manifest, wrangler)?;
    copy_file(
        artifact_path(pack_root, manifest, &ArtifactKind::RuntimeExecutable)?,
        output.join(format!("{app_name}.exe")),
    )?;
    let config = ShellConfig {
        name: app_name,
        host: format!("{app_name}.appd.local"),
    };
    fs::write(
        output.join("appd.json"),
        serde_json::to_vec_pretty(&config)?,
    )
    .with_context(|| format!("write Windows shell configuration in {}", output.display()))?;

    Ok(BuildSummary {
        platform: BuildPlatform::Windows,
        bundle_dir: output,
    })
}
