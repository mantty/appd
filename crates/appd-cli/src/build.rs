use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_runtime::assets::{PACKAGED_ASSETS_DIR_NAME, write_runtime_assets};
use appd_runtime::workerd_config::{ConfigOptions, generate};
use appd_runtime::wrangler_config::{WranglerConfig, load_config, resolve_config_path};
use appd_target_pack::{ArtifactKind, Target, TargetPackManifest, load_manifest};
use walkdir::WalkDir;

use crate::{BuildPlatform, target_packs};

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
    if request.platforms.is_empty() {
        bail!("at least one build platform is required");
    }
    if request.target_pack_manifest.is_some() && request.platforms.len() > 1 {
        bail!("--target-pack can only be used with a single platform");
    }

    let project_dir = request.project_dir.clone();
    if !project_dir.is_dir() {
        bail!(
            "project directory does not exist: {}",
            project_dir.display()
        );
    }
    let config_reference_dir = if request.config_path.is_some() {
        std::env::current_dir()?
    } else {
        project_dir.clone()
    };
    let config_path = resolve_config_path(&config_reference_dir, request.config_path.as_deref())?;
    let wrangler_config = load_config(&config_path)?;

    if !request.skip_web_build {
        run_web_build(&project_dir)?;
    }
    ensure_worker_build_exists(&wrangler_config)?;

    let app_name = read_package_name(&project_dir).unwrap_or_else(|_| "appdapp".to_owned());

    let mut summaries = Vec::with_capacity(request.platforms.len());
    for platform in &request.platforms {
        let target_pack_manifest =
            target_packs::resolve_manifest(*platform, request.target_pack_manifest.as_deref())?;
        let manifest = load_manifest(&target_pack_manifest)
            .with_context(|| format!("invalid target pack: {}", target_pack_manifest.display()))?;
        ensure_pack_matches_platform(&manifest, *platform)?;
        let pack_root = target_pack_manifest
            .parent()
            .context("target pack manifest must have a parent directory")?;

        let summary = match platform {
            BuildPlatform::Linux => build_linux(
                &project_dir,
                pack_root,
                &manifest,
                &app_name,
                *platform,
                &wrangler_config,
            ),
            BuildPlatform::Windows => build_windows(
                &project_dir,
                pack_root,
                &manifest,
                &app_name,
                *platform,
                &wrangler_config,
            ),
            BuildPlatform::Macos => build_macos(
                &project_dir,
                pack_root,
                &manifest,
                &app_name,
                *platform,
                &wrangler_config,
            ),
            BuildPlatform::Ios | BuildPlatform::IosSimulator => build_ios(
                &project_dir,
                pack_root,
                &manifest,
                &app_name,
                *platform,
                &wrangler_config,
            ),
            BuildPlatform::Android => build_android(
                &project_dir,
                pack_root,
                &manifest,
                &app_name,
                *platform,
                &wrangler_config,
            ),
        }?;
        summaries.push(summary);
    }

    Ok(summaries)
}

fn build_linux(
    project_dir: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    platform: BuildPlatform,
    wrangler_config: &WranglerConfig,
) -> Result<BuildSummary> {
    let output_dir = platform_build_dir(project_dir, platform).join(app_name);
    reset_path(&output_dir)?;
    let app_dir = output_dir.join("app");
    fs::create_dir_all(&app_dir)?;

    let runtime = artifact_path(pack_root, manifest, &ArtifactKind::RuntimeExecutable)?;
    let exe = output_dir.join(app_name);
    copy_file(&runtime, &exe)?;
    make_executable(&exe)?;
    prepare_workerd_app(&app_dir, wrangler_config, false)?;

    Ok(BuildSummary {
        platform,
        bundle_dir: output_dir,
    })
}

fn build_windows(
    project_dir: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    platform: BuildPlatform,
    wrangler_config: &WranglerConfig,
) -> Result<BuildSummary> {
    let output_dir = platform_build_dir(project_dir, platform).join(app_name);
    reset_path(&output_dir)?;
    let app_dir = output_dir.join("app");
    fs::create_dir_all(&app_dir)?;

    let runtime = artifact_path(pack_root, manifest, &ArtifactKind::RuntimeExecutable)?;
    copy_file(&runtime, output_dir.join(format!("{app_name}.exe")))?;
    prepare_workerd_app(&app_dir, wrangler_config, false)?;

    Ok(BuildSummary {
        platform,
        bundle_dir: output_dir,
    })
}

fn build_macos(
    project_dir: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    platform: BuildPlatform,
    wrangler_config: &WranglerConfig,
) -> Result<BuildSummary> {
    let bundle_dir = platform_build_dir(project_dir, platform).join(format!("{app_name}.app"));
    reset_path(&bundle_dir)?;

    let contents = bundle_dir.join("Contents");
    let macos_dir = contents.join("MacOS");
    let app_dir = contents.join("Resources").join("app");
    fs::create_dir_all(&macos_dir)?;
    fs::create_dir_all(&app_dir)?;

    let runtime = artifact_path(pack_root, manifest, &ArtifactKind::RuntimeExecutable)?;
    let exe = macos_dir.join(app_name);
    copy_file(&runtime, &exe)?;
    make_executable(&exe)?;
    prepare_workerd_app(&app_dir, wrangler_config, false)?;
    write_macos_plist(&contents.join("Info.plist"), &bundle_id(app_name), app_name)?;
    ad_hoc_codesign(&bundle_dir)?;

    Ok(BuildSummary {
        platform,
        bundle_dir,
    })
}

fn build_ios(
    project_dir: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    platform: BuildPlatform,
    wrangler_config: &WranglerConfig,
) -> Result<BuildSummary> {
    let bundle_dir = platform_build_dir(project_dir, platform).join(format!("{app_name}.app"));
    reset_path(&bundle_dir)?;
    let app_dir = bundle_dir.join("app");
    fs::create_dir_all(&app_dir)?;

    let runtime = artifact_path(pack_root, manifest, &ArtifactKind::RuntimeExecutable)?;
    let exe = bundle_dir.join(app_name);
    copy_file(&runtime, &exe)?;
    make_executable(&exe)?;
    prepare_workerd_app(&app_dir, wrangler_config, true)?;
    write_ios_plist(
        &bundle_dir.join("Info.plist"),
        &bundle_id(app_name),
        app_name,
    )?;
    ad_hoc_codesign(&bundle_dir)?;

    Ok(BuildSummary {
        platform,
        bundle_dir,
    })
}

fn build_android(
    project_dir: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    platform: BuildPlatform,
    wrangler_config: &WranglerConfig,
) -> Result<BuildSummary> {
    let stage_dir = platform_build_dir(project_dir, platform).join("stage");
    reset_path(&stage_dir)?;

    let app_dir = stage_dir.join("assets").join("app");
    let lib_dir = stage_dir.join("lib").join("arm64-v8a");
    fs::create_dir_all(&app_dir)?;
    fs::create_dir_all(&lib_dir)?;

    let runtime = artifact_path(pack_root, manifest, &ArtifactKind::RuntimeSharedLibrary)?;
    copy_file(&runtime, lib_dir.join("libappd.so"))?;

    let dex = artifact_path(pack_root, manifest, &ArtifactKind::AndroidDex)?;
    copy_file(&dex, stage_dir.join("classes.dex"))?;

    let manifest_template = artifact_path(pack_root, manifest, &ArtifactKind::AndroidManifest)?;
    let manifest_content = fs::read_to_string(&manifest_template)
        .with_context(|| format!("read {}", manifest_template.display()))?;
    let package_name = android_package_name(app_name);
    let manifest_content = manifest_content
        .replace("__PACKAGE__", &package_name)
        .replace("__APP_NAME__", app_name);
    fs::write(stage_dir.join("AndroidManifest.xml"), manifest_content)?;

    if let Some(resource_dir) =
        optional_artifact_path(pack_root, manifest, &ArtifactKind::ResourceDirectory)
    {
        copy_dir_contents(&resource_dir, &stage_dir)?;
    }

    prepare_workerd_app(&app_dir, wrangler_config, true)?;

    Ok(BuildSummary {
        platform,
        bundle_dir: stage_dir,
    })
}

fn prepare_workerd_app(
    app_dir: &Path,
    wrangler_config: &WranglerConfig,
    jitless: bool,
) -> Result<()> {
    let worker_source_dir = wrangler_config
        .main
        .parent()
        .context("worker main must have a parent directory")?;
    let worker_main_module = wrangler_config.main.strip_prefix(worker_source_dir)?;

    copy_dir_contents(worker_source_dir, &app_dir.join("worker"))?;
    if let Some(assets) = wrangler_config.assets.as_ref() {
        copy_dir_contents(&assets.directory, &app_dir.join(PACKAGED_ASSETS_DIR_NAME))?;
    }

    write_runtime_assets(app_dir)?;
    generate(&ConfigOptions {
        work_dir: app_dir.to_path_buf(),
        worker_dir: PathBuf::from("worker"),
        worker_main_module: slash_path(worker_main_module)?,
        wrangler_config: wrangler_config.clone(),
        jitless,
    })?;
    Ok(())
}

fn ensure_pack_matches_platform(
    manifest: &TargetPackManifest,
    platform: BuildPlatform,
) -> Result<()> {
    let matches = match platform {
        BuildPlatform::Ios => matches!(
            manifest.target,
            Target::IosArm64 | Target::IosSimulatorArm64
        ),
        BuildPlatform::IosSimulator => manifest.target == Target::IosSimulatorArm64,
        BuildPlatform::Android => manifest.target == Target::AndroidArm64,
        BuildPlatform::Macos => matches!(manifest.target, Target::MacosArm64 | Target::MacosX64),
        BuildPlatform::Windows => manifest.target == Target::WindowsX64,
        BuildPlatform::Linux => manifest.target == Target::LinuxX64,
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

fn platform_build_dir(project_dir: &Path, platform: BuildPlatform) -> PathBuf {
    project_dir.join("build").join(platform.build_dir_name())
}

fn artifact_path(
    pack_root: &Path,
    manifest: &TargetPackManifest,
    kind: &ArtifactKind,
) -> Result<PathBuf> {
    optional_artifact_path(pack_root, manifest, kind)
        .with_context(|| format!("target pack missing {kind:?} artifact"))
}

fn optional_artifact_path(
    pack_root: &Path,
    manifest: &TargetPackManifest,
    kind: &ArtifactKind,
) -> Option<PathBuf> {
    manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == *kind)
        .map(|artifact| pack_root.join(&artifact.path))
}

fn run_web_build(project_dir: &Path) -> Result<()> {
    let package_json = project_dir.join("package.json");
    if !package_json.is_file() {
        bail!("package.json not found in {}", project_dir.display());
    }

    let (program, args): (&str, &[&str]) = if project_dir.join("pnpm-lock.yaml").is_file() {
        ("pnpm", &["run", "build"])
    } else if project_dir.join("yarn.lock").is_file() {
        ("yarn", &["build"])
    } else {
        ("npm", &["run", "build"])
    };

    let status = Command::new(program)
        .args(args)
        .current_dir(project_dir)
        .status()
        .with_context(|| format!("failed to run {program}"))?;

    if status.success() {
        Ok(())
    } else {
        bail!("{program} build failed with status {status}");
    }
}

fn ensure_worker_build_exists(wrangler_config: &WranglerConfig) -> Result<()> {
    if !wrangler_config.main.is_file() {
        bail!("worker main not found: {}", wrangler_config.main.display());
    }

    if let Some(assets) = wrangler_config.assets.as_ref()
        && !assets.directory.is_dir()
    {
        bail!("assets directory not found: {}", assets.directory.display());
    }

    Ok(())
}

fn read_package_name(project_dir: &Path) -> Result<String> {
    let content = fs::read_to_string(project_dir.join("package.json"))?;
    let json: serde_json::Value = serde_json::from_str(&content)?;
    let name = json
        .get("name")
        .and_then(serde_json::Value::as_str)
        .context("package.json name must be a string")?;

    validate_app_name(name)?;
    Ok(name.to_owned())
}

fn validate_app_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || name.contains(':')
    {
        bail!("package.json name is not a safe app name: {name}");
    }
    Ok(())
}

fn reset_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn copy_file(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    let from = from.as_ref();
    let to = to.as_ref();
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to).with_context(|| format!("copy {} to {}", from.display(), to.display()))?;
    Ok(())
}

fn copy_dir_contents(from: &Path, to: &Path) -> Result<()> {
    if !from.is_dir() {
        bail!("directory not found: {}", from.display());
    }

    for entry in WalkDir::new(from) {
        let entry = entry.map_err(std::io::Error::other)?;
        let rel = entry.path().strip_prefix(from)?;
        if rel.as_os_str().is_empty() {
            continue;
        }

        let dest = to.join(rel);
        let file_type = entry.file_type();
        if file_type.is_dir() {
            fs::create_dir_all(&dest)?;
        } else if file_type.is_file() {
            copy_file(entry.path(), dest)?;
        }
    }

    Ok(())
}

fn slash_path(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .with_context(|| format!("path is not valid UTF-8: {}", path.display()))?;
    Ok(path.replace(std::path::MAIN_SEPARATOR, "/"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

fn write_macos_plist(path: &Path, bundle_id: &str, app_name: &str) -> Result<()> {
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>{bundle_id}</string>
  <key>CFBundleName</key>
  <string>{app_name}</string>
  <key>CFBundleExecutable</key>
  <string>{app_name}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
"#
    );
    fs::write(path, content)?;
    Ok(())
}

fn write_ios_plist(path: &Path, bundle_id: &str, app_name: &str) -> Result<()> {
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>{bundle_id}</string>
  <key>CFBundleName</key>
  <string>{app_name}</string>
  <key>CFBundleExecutable</key>
  <string>{app_name}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0</string>
  <key>MinimumOSVersion</key>
  <string>17.0</string>
  <key>LSRequiresIPhoneOS</key>
  <true/>
  <key>UIDeviceFamily</key>
  <array>
    <integer>1</integer>
    <integer>2</integer>
  </array>
  <key>UISupportedInterfaceOrientations</key>
  <array>
    <string>UIInterfaceOrientationPortrait</string>
    <string>UIInterfaceOrientationLandscapeLeft</string>
    <string>UIInterfaceOrientationLandscapeRight</string>
  </array>
  <key>UILaunchScreen</key>
  <dict/>
  <key>NSAppTransportSecurity</key>
  <dict>
    <key>NSAllowsArbitraryLoads</key>
    <true/>
  </dict>
</dict>
</plist>
"#
    );
    fs::write(path, content)?;
    Ok(())
}

fn ad_hoc_codesign(path: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }

    let status = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(path)
        .status()
        .context("failed to run codesign")?;
    if status.success() {
        Ok(())
    } else {
        bail!("codesign failed with status {status}");
    }
}

fn bundle_id(app_name: &str) -> String {
    format!("com.appd.{}", app_name.replace('-', "_"))
}

fn android_package_name(app_name: &str) -> String {
    format!("com.appd.{}", app_name.replace('-', "_").to_lowercase())
}
