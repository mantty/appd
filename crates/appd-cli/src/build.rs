use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use appd_runtime::wrangler_config::{WranglerConfig, load_config, resolve_config_path};
use appd_target_pack::load_manifest;

use crate::{BuildPlatform, target_packs};

#[path = "apple.rs"]
mod apple;
#[path = "support.rs"]
mod support;
#[path = "worker.rs"]
mod worker;

pub(crate) struct BuildRequest {
    pub(crate) platforms: Vec<BuildPlatform>,
    pub(crate) project_dir: PathBuf,
    pub(crate) target_pack_manifest: Option<PathBuf>,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) skip_web_build: bool,
}

pub(crate) struct BuildSummary {
    pub(crate) platform: BuildPlatform,
    pub(crate) bundle_dir: PathBuf,
}

pub(crate) fn run(request: &BuildRequest) -> Result<Vec<BuildSummary>> {
    validate_request(request)?;
    if !request.skip_web_build {
        support::run_web_build(&request.project_dir)?;
    }
    let config_base = if request.config_path.is_some() {
        std::env::current_dir()?
    } else {
        request.project_dir.clone()
    };
    let config_path = resolve_config_path(&config_base, request.config_path.as_deref())?;
    let wrangler = load_config(&config_path)?;
    support::validate_web_build(&wrangler)?;
    let app_name = support::read_package_name(&request.project_dir)?;

    request
        .platforms
        .iter()
        .map(|platform| build_platform(request, *platform, &app_name, &wrangler))
        .collect()
}

fn validate_request(request: &BuildRequest) -> Result<()> {
    if request.platforms.is_empty() {
        bail!("at least one build platform is required");
    }
    if !request.project_dir.is_dir() {
        bail!(
            "project directory does not exist: {}",
            request.project_dir.display()
        );
    }
    if request.target_pack_manifest.is_some() && request.platforms.len() > 1 {
        bail!("--target-pack can only be used with a single platform");
    }
    Ok(())
}

fn build_platform(
    request: &BuildRequest,
    platform: BuildPlatform,
    app_name: &str,
    wrangler: &WranglerConfig,
) -> Result<BuildSummary> {
    let manifest_path = fs::canonicalize(target_packs::resolve_manifest(
        platform,
        request.target_pack_manifest.as_deref(),
    )?)?;
    let manifest = load_manifest(&manifest_path)
        .with_context(|| format!("invalid target pack: {}", manifest_path.display()))?;
    support::validate_target(&manifest, platform)?;
    let pack_root = manifest_path
        .parent()
        .context("target-pack manifest must have a parent directory")?;

    match platform {
        BuildPlatform::Macos => apple::build_macos(
            &request.project_dir,
            pack_root,
            &manifest,
            app_name,
            wrangler,
        ),
        BuildPlatform::Ios => apple::build_ios(
            &request.project_dir,
            pack_root,
            &manifest,
            app_name,
            wrangler,
        ),
    }
}
