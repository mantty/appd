use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use appd::{WranglerConfig, load_wrangler_config, resolve_wrangler_config_path};
use appd_cli::{MANIFEST_FILE, Platform, Target, TargetPackManifest, load_manifest};

use super::{plugins, support, worker};

pub(crate) struct BuildRequest {
    pub(crate) platforms: Vec<Platform>,
    pub(crate) project_dir: PathBuf,
    pub(crate) target_pack_manifest: Option<PathBuf>,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) skip_web_build: bool,
}

pub(crate) struct BuildSummary {
    pub(crate) platform: Platform,
    pub(crate) bundle_dir: PathBuf,
}

pub(crate) fn run(request: &BuildRequest) -> Result<Vec<BuildSummary>> {
    validate_request(request)?;
    if !request.skip_web_build {
        support::run_web_build(&request.project_dir)?;
    }
    let config_base = if request.config_path.is_some() {
        env::current_dir()?
    } else {
        request.project_dir.clone()
    };
    let config_path = resolve_wrangler_config_path(&config_base, request.config_path.as_deref())?;
    let wrangler = load_wrangler_config(&config_path)?;
    support::validate_web_build(&wrangler)?;
    let app_name = support::read_package_name(&request.project_dir)?;
    let plugins = plugins::discover(&request.project_dir)?;

    request
        .platforms
        .iter()
        .map(|platform| build_platform(request, *platform, &app_name, &wrangler, &plugins))
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
    platform: Platform,
    app_name: &str,
    wrangler: &WranglerConfig,
    plugins: &[plugins::Plugin],
) -> Result<BuildSummary> {
    let manifest_path = fs::canonicalize(resolve_manifest(
        platform,
        request.target_pack_manifest.as_deref(),
    )?)?;
    let manifest = load_manifest(&manifest_path)
        .with_context(|| format!("invalid target pack: {}", manifest_path.display()))?;
    manifest
        .validate_cli_version(env!("CARGO_PKG_VERSION"))
        .with_context(|| format!("incompatible target pack: {}", manifest_path.display()))?;
    support::validate_target(&manifest, platform)?;
    let pack_root = manifest_path
        .parent()
        .context("target-pack manifest must have a parent directory")?;
    let project = fs::canonicalize(&request.project_dir).with_context(|| {
        format!(
            "resolve project directory: {}",
            request.project_dir.display()
        )
    })?;
    let staging = project.join("build/.appd").join(platform.directory_name());
    support::reset_path(&staging)
        .with_context(|| format!("reset build staging directory: {}", staging.display()))?;
    let input = staging.join("input");
    fs::create_dir_all(input.join("metadata")).with_context(|| {
        format!(
            "create build input directory: {}",
            input.join("metadata").display()
        )
    })?;
    fs::create_dir_all(input.join("app"))?;
    support::stage_platform_artifacts(&input, pack_root, &manifest)
        .context("stage target-pack platform artifacts")?;

    worker::prepare_quickjs_app(&input.join("app"), pack_root, &manifest, wrangler)
        .context("prepare the appd application package")?;
    plugins::stage(plugins, platform, &input.join("plugins"))
        .context("stage native plugin inputs")?;
    write_build_metadata(&input, app_name, platform, &manifest)
        .context("write platform build metadata")?;

    let output = output_path(&project, platform, app_name);
    support::run_entrypoint(pack_root, &input, &output, manifest.target).with_context(|| {
        format!(
            "build {} using target-pack entrypoint",
            platform.display_name()
        )
    })?;
    Ok(BuildSummary {
        platform,
        bundle_dir: output,
    })
}

fn output_path(project: &Path, platform: Platform, app_name: &str) -> PathBuf {
    support::build_dir(project, platform).join(platform.output_name(app_name))
}

fn write_build_metadata(
    input: &Path,
    app_name: &str,
    platform: Platform,
    manifest: &TargetPackManifest,
) -> Result<()> {
    let metadata = input.join("metadata");
    let values = [
        ("app-name", app_name.to_owned()),
        ("bundle-id", format!("com.appd.{app_name}")),
        ("host", format!("{app_name}.appd.local")),
        ("platform", platform.directory_name().to_owned()),
        ("target", manifest.target.to_string()),
    ];
    for (name, value) in values {
        fs::write(metadata.join(name), value)?;
    }
    Ok(())
}

const TARGET_PACK_DIR_ENV: &str = "target_pack_dir";

fn resolve_manifest(platform: Platform, explicit_manifest: Option<&Path>) -> Result<PathBuf> {
    if let Some(manifest) = explicit_manifest {
        return Ok(manifest.to_path_buf());
    }

    let target = platform.default_target().map_err(anyhow::Error::from)?;
    if let Some(root) = env::var_os(TARGET_PACK_DIR_ENV) {
        return manifest_from_root(Path::new(&root), target).with_context(|| {
            format!("{TARGET_PACK_DIR_ENV} does not contain a target pack for {target}")
        });
    }

    if let Some(manifest) = bundled_manifest(target) {
        return Ok(manifest);
    }

    bail!(
        "no target pack found for {target}; pass --target-pack or build one with `cargo run -p xtask -- target-pack --target {target}`"
    )
}

fn manifest_from_root(root: &Path, target: Target) -> Result<PathBuf> {
    let manifest = root.join(target.to_string()).join(MANIFEST_FILE);
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
        let manifest = root.join(target.to_string()).join(MANIFEST_FILE);
        if manifest.is_file() {
            return Some(manifest);
        }
    }
    None
}
