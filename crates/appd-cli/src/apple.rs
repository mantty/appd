use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_bundle::wrangler::WranglerConfig;
use appd_target_pack::{Target, TargetPackManifest};

use super::plugins::Plugin;
use super::support::{artifact_path, build_dir, copy_file, make_executable, reset_path};
use super::worker::prepare_bare_app;
use super::{BuildPlatform, BuildSummary};

pub(crate) fn build_macos(
    project: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    wrangler: &WranglerConfig,
    plugins: &[Plugin],
) -> Result<BuildSummary> {
    let output = build_dir(project, BuildPlatform::Macos);
    let bundle = output.join(format!("{app_name}.app"));
    reset_path(&bundle)?;
    let contents = bundle.join("Contents");
    let executable_dir = contents.join("MacOS");
    let app_dir = contents.join("Resources/app");
    fs::create_dir_all(&executable_dir)?;
    fs::create_dir_all(&app_dir)?;

    prepare_bare_app(&app_dir, pack_root, manifest, wrangler)?;
    write_macos_plist(
        &contents.join("Info.plist"),
        &bundle_id(app_name),
        app_name,
        plugins,
    )?;
    compile_shell(
        pack_root,
        manifest,
        &executable_dir.join(app_name),
        &output.join(".appd/module-cache"),
        BuildPlatform::Macos,
        plugins,
    )?;
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
    plugins: &[Plugin],
) -> Result<BuildSummary> {
    let platform = ios_build_platform(manifest.target)?;
    let output = build_dir(project, platform);
    let bundle = output.join(format!("{app_name}.app"));
    reset_path(&bundle)?;
    let app_dir = bundle.join("app");
    fs::create_dir_all(&app_dir)?;

    prepare_bare_app(&app_dir, pack_root, manifest, wrangler)?;
    write_ios_plist(
        &bundle.join("Info.plist"),
        &bundle_id(app_name),
        app_name,
        platform == BuildPlatform::IosSimulator,
        platform,
        plugins,
    )?;
    compile_shell(
        pack_root,
        manifest,
        &bundle.join(app_name),
        &output.join(".appd/module-cache"),
        platform,
        plugins,
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

fn compile_shell(
    pack_root: &Path,
    manifest: &TargetPackManifest,
    output: &Path,
    module_cache: &Path,
    platform: BuildPlatform,
    plugins: &[Plugin],
) -> Result<()> {
    let framework = artifact_path(
        pack_root,
        manifest,
        &appd_target_pack::ArtifactKind::RuntimeLibrary,
    )?;
    let sources = artifact_path(
        pack_root,
        manifest,
        &appd_target_pack::ArtifactKind::NativeShellDirectory,
    )?;
    fs::create_dir_all(module_cache)?;
    let framework_root = framework
        .parent()
        .context("runtime framework must have a parent directory")?;
    let registry = module_cache.join("AppdPluginRegistry.swift");
    fs::write(&registry, plugin_registry(plugins, platform))?;
    let swift_sources = swift_sources(&sources, plugins, platform)?;

    let mut command = Command::new("xcrun");
    command
        .args(["--sdk", apple_sdk(manifest.target), "swiftc"])
        .args(swift_sources)
        .arg(registry)
        .args(["-target", apple_target(manifest.target)])
        .args([
            "-swift-version",
            "5",
            "-Osize",
            "-whole-module-optimization",
        ])
        .args(["-module-cache-path"])
        .arg(module_cache)
        .arg("-F")
        .arg(framework_root)
        .args([
            "-framework",
            "AppdRuntime",
            "-framework",
            "JavaScriptCore",
            "-framework",
            "CoreFoundation",
            "-lc++",
            "-lresolv",
            "-Xlinker",
            "-dead_strip",
        ]);
    for framework in plugin_frameworks(plugins, platform) {
        command.args(["-framework", &framework]);
    }
    if matches!(manifest.target, Target::MacosArm64 | Target::MacosX64) {
        command.args(["-Xlinker", "-export_dynamic"]);
    }
    let status = command
        .args(["-o"])
        .arg(output)
        .status()
        .context("failed to compile the Apple application shell")?;
    if !status.success() {
        bail!("Apple application shell build failed with status {status}");
    }
    make_executable(output)
}

fn swift_sources(
    shell: &Path,
    plugins: &[Plugin],
    platform: BuildPlatform,
) -> Result<Vec<PathBuf>> {
    let mut sources = fs::read_dir(shell)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "swift")
        })
        .collect::<Vec<_>>();
    for plugin in plugins {
        for source in plugin.sources(platform)? {
            if source
                .extension()
                .is_none_or(|extension| extension != "swift")
            {
                bail!(
                    "Apple plugin '{}' source must be Swift: {}",
                    plugin.id,
                    source.display()
                );
            }
            sources.push(source);
        }
    }
    sources.sort();
    if !sources.iter().any(|source| {
        source
            .file_name()
            .is_some_and(|name| name == "AppdShell.swift")
    }) {
        bail!("Apple native shell source is missing: {}", shell.display());
    }
    Ok(sources)
}

fn plugin_registry(plugins: &[Plugin], platform: BuildPlatform) -> String {
    let instances = plugins
        .iter()
        .filter_map(|plugin| plugin.platform(platform))
        .map(|native| format!("    {}(),", native.class))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "import Foundation\n\nfunc appdPlugins() -> [any AppdPlugin] {{\n  [\n{instances}\n  ]\n}}\n"
    )
}

fn plugin_frameworks(plugins: &[Plugin], platform: BuildPlatform) -> Vec<String> {
    plugins
        .iter()
        .filter_map(|plugin| plugin.platform(platform))
        .flat_map(|native| native.frameworks.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn apple_sdk(target: Target) -> &'static str {
    match target {
        Target::MacosArm64 | Target::MacosX64 => "macosx",
        Target::IosArm64 => "iphoneos",
        Target::IosSimulatorArm64 | Target::IosSimulatorX64 => "iphonesimulator",
        Target::AndroidArm64 => unreachable!("Android does not use the Apple SDK"),
    }
}

fn apple_target(target: Target) -> &'static str {
    match target {
        Target::MacosArm64 => "arm64-apple-macos14.0",
        Target::MacosX64 => "x86_64-apple-macos14.0",
        Target::IosArm64 => "arm64-apple-ios17.0",
        Target::IosSimulatorArm64 => "arm64-apple-ios17.0-simulator",
        Target::IosSimulatorX64 => "x86_64-apple-ios17.0-simulator",
        Target::AndroidArm64 => unreachable!("Android does not use an Apple target"),
    }
}

fn write_macos_plist(
    path: &Path,
    identifier: &str,
    app_name: &str,
    plugins: &[Plugin],
) -> Result<()> {
    let plugin_entries = plugin_plist_entries(plugins, BuildPlatform::Macos)?;
    fs::write(
        path,
        plist(identifier, app_name, macos_plist_entries(), &plugin_entries),
    )?;
    Ok(())
}

fn write_ios_plist(
    path: &Path,
    identifier: &str,
    app_name: &str,
    simulator: bool,
    platform: BuildPlatform,
    plugins: &[Plugin],
) -> Result<()> {
    let supported_platform = if simulator {
        "iPhoneSimulator"
    } else {
        "iPhoneOS"
    };
    let plugin_entries = plugin_plist_entries(plugins, platform)?;
    fs::write(
        path,
        plist(
            identifier,
            app_name,
            &ios_plist_entries(supported_platform),
            &plugin_entries,
        ),
    )?;
    Ok(())
}

fn plist(identifier: &str, app_name: &str, platform: &str, plugins: &str) -> String {
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
  <key>AppdHost</key><string>{app_name}.appd.local</string>
  {plugins}
  {platform}
</dict></plist>
"#
    )
}

fn plugin_plist_entries(plugins: &[Plugin], platform: BuildPlatform) -> Result<String> {
    let mut entries = std::collections::BTreeMap::new();
    for native in plugins
        .iter()
        .filter_map(|plugin| plugin.platform(platform))
    {
        for (key, value) in &native.plist {
            if let Some(existing) = entries.insert(key, value)
                && existing != value
            {
                bail!("plugins define conflicting values for Apple plist key '{key}'");
            }
        }
    }
    Ok(entries
        .into_iter()
        .map(|(key, value)| {
            format!(
                "<key>{}</key><string>{}</string>",
                xml_escape(key),
                xml_escape(value)
            )
        })
        .collect::<Vec<_>>()
        .join("\n  "))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

    #[test]
    fn apple_bundles_use_the_named_stable_origin() {
        let plist = super::plist(
            "com.appd.example",
            "example",
            super::macos_plist_entries(),
            "",
        );

        assert!(plist.contains("<key>AppdHost</key><string>example.appd.local</string>"));
    }
}
