use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_runtime::wrangler_config::WranglerConfig;
use appd_target_pack::{Target, TargetPackManifest};

use super::support::{build_dir, copy_file, make_executable, reset_path};
use super::worker::prepare_bare_app;
use super::{BuildPlatform, BuildSummary};

pub(crate) fn build_macos(
    project: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    wrangler: &WranglerConfig,
) -> Result<BuildSummary> {
    let bundle = build_dir(project, BuildPlatform::Macos).join(format!("{app_name}.app"));
    reset_path(&bundle)?;
    let contents = bundle.join("Contents");
    let executable_dir = contents.join("MacOS");
    let app_dir = contents.join("Resources/app");
    fs::create_dir_all(&executable_dir)?;
    fs::create_dir_all(&app_dir)?;

    install_runtime(pack_root, manifest, &executable_dir.join(app_name))?;
    prepare_bare_app(&app_dir, pack_root, manifest, wrangler)?;
    write_macos_plist(&contents.join("Info.plist"), &bundle_id(app_name), app_name)?;
    sign(&bundle, "-", None)?;
    Ok(BuildSummary {
        platform: BuildPlatform::Macos,
        bundle_dir: bundle,
    })
}

pub(crate) fn build_ios(
    project: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    wrangler: &WranglerConfig,
) -> Result<BuildSummary> {
    let platform = ios_build_platform(manifest.target)?;
    let bundle = build_dir(project, platform).join(format!("{app_name}.app"));
    reset_path(&bundle)?;
    let app_dir = bundle.join("app");
    fs::create_dir_all(&app_dir)?;

    install_runtime(pack_root, manifest, &bundle.join(app_name))?;
    prepare_bare_app(&app_dir, pack_root, manifest, wrangler)?;
    write_ios_plist(
        &bundle.join("Info.plist"),
        &bundle_id(app_name),
        app_name,
        platform == BuildPlatform::IosSimulator,
    )?;
    if platform == BuildPlatform::IosSimulator {
        sign(&bundle, "-", None)?;
    } else {
        sign_ios(&bundle)?;
    }
    Ok(BuildSummary {
        platform,
        bundle_dir: bundle,
    })
}

fn ios_build_platform(target: Target) -> Result<BuildPlatform> {
    match target {
        Target::IosArm64 => Ok(BuildPlatform::Ios),
        Target::IosSimulatorArm64 | Target::IosSimulatorX64 => Ok(BuildPlatform::IosSimulator),
        _ => bail!("target pack {target} is not an iOS target"),
    }
}

fn install_runtime(pack_root: &Path, manifest: &TargetPackManifest, to: &Path) -> Result<()> {
    let runtime = super::support::artifact_path(
        pack_root,
        manifest,
        &appd_target_pack::ArtifactKind::RuntimeExecutable,
    )?;
    copy_file(runtime, to)?;
    make_executable(to)
}

fn write_macos_plist(path: &Path, identifier: &str, app_name: &str) -> Result<()> {
    fs::write(path, plist(identifier, app_name, macos_plist_entries()))?;
    Ok(())
}

fn write_ios_plist(path: &Path, identifier: &str, app_name: &str, simulator: bool) -> Result<()> {
    let supported_platform = if simulator {
        "iPhoneSimulator"
    } else {
        "iPhoneOS"
    };
    fs::write(
        path,
        plist(identifier, app_name, &ios_plist_entries(supported_platform)),
    )?;
    Ok(())
}

fn plist(identifier: &str, app_name: &str, platform: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>{identifier}</string>
  <key>CFBundleName</key><string>{app_name}</string>
  <key>CFBundleExecutable</key><string>{app_name}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  {platform}
</dict></plist>
"#
    )
}

fn ios_plist_entries(platform: &str) -> String {
    format!(
        r"<key>CFBundleSupportedPlatforms</key><array><string>{platform}</string></array>
  <key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
  <key>MinimumOSVersion</key><string>17.0</string>
  <key>LSRequiresIPhoneOS</key><true/>
  <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
  <key>UILaunchScreen</key><dict/>
  <key>UISupportedInterfaceOrientations</key><array>
    <string>UIInterfaceOrientationPortrait</string>
    <string>UIInterfaceOrientationLandscapeLeft</string>
    <string>UIInterfaceOrientationLandscapeRight</string>
  </array>"
    )
}

fn macos_plist_entries() -> &'static str {
    "<key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
  <key>NSHighResolutionCapable</key><true/>"
}

fn sign_ios(bundle: &Path) -> Result<()> {
    let identity = std::env::var("APPD_IOS_SIGNING_IDENTITY").ok();
    let profile = std::env::var_os("APPD_IOS_PROVISIONING_PROFILE").map(PathBuf::from);
    match (identity, profile) {
        (None, None) => sign(bundle, "-", None),
        (Some(identity), Some(profile)) => sign_ios_for_device(bundle, &identity, &profile),
        _ => bail!(
            "physical iOS signing requires APPD_IOS_SIGNING_IDENTITY and APPD_IOS_PROVISIONING_PROFILE"
        ),
    }
}

fn sign_ios_for_device(bundle: &Path, identity: &str, profile: &Path) -> Result<()> {
    copy_file(profile, bundle.join("embedded.mobileprovision"))?;
    let entitlements = bundle.with_extension("entitlements.plist");
    extract_entitlements(profile, &entitlements)?;
    let result = sign(bundle, identity, Some(&entitlements));
    fs::remove_file(entitlements)?;
    result
}

fn extract_entitlements(profile: &Path, output: &Path) -> Result<()> {
    let decoded = decode_profile(profile)?;
    let source = output.with_extension("profile.plist");
    fs::write(&source, decoded)?;
    let status = Command::new("plutil")
        .args(["-extract", "Entitlements", "xml1", "-o"])
        .arg(output)
        .arg(&source)
        .status()
        .context("failed to extract iOS signing entitlements");
    fs::remove_file(source)?;
    let status = status?;
    if status.success() {
        Ok(())
    } else {
        bail!("failed to extract iOS signing entitlements")
    }
}

fn decode_profile(profile: &Path) -> Result<Vec<u8>> {
    let security = Command::new("security")
        .args(["cms", "-D", "-i"])
        .arg(profile)
        .output()
        .context("failed to decode the iOS provisioning profile")?;
    if security.status.success() {
        return Ok(security.stdout);
    }
    let openssl = Command::new("openssl")
        .args(["smime", "-inform", "der", "-verify", "-noverify", "-in"])
        .arg(profile)
        .output()
        .context("failed to run the provisioning profile decoder")?;
    if openssl.status.success() {
        return Ok(openssl.stdout);
    }
    bail!("failed to decode the iOS provisioning profile")
}

fn sign(path: &Path, identity: &str, entitlements: Option<&Path>) -> Result<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let mut command = Command::new("codesign");
    command.args(["--force", "--timestamp=none", "--sign", identity]);
    if let Some(entitlements) = entitlements {
        command
            .arg("--generate-entitlement-der")
            .arg("--entitlements")
            .arg(entitlements);
    }
    let status = command
        .arg(path)
        .status()
        .context("failed to run codesign")?;
    if status.success() {
        Ok(())
    } else {
        bail!("codesign failed with status {status}")
    }
}

fn bundle_id(app_name: &str) -> String {
    format!("com.appd.{app_name}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn apple_bundles_allow_local_networking() {
        assert!(super::ios_plist_entries("iPhoneOS").contains("NSAllowsLocalNetworking"));
        assert!(super::macos_plist_entries().contains("NSAllowsLocalNetworking"));
    }

    #[test]
    fn simulator_bundles_use_the_simulator_platform() {
        assert!(super::ios_plist_entries("iPhoneSimulator").contains("iPhoneSimulator"));
    }
}
