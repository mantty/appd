use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_bundle::wrangler::WranglerConfig;
use appd_target_pack::{ArtifactKind, TargetPackManifest};

use super::plugins::Plugin;
use super::support::{artifact_path, build_dir, copy_dir_contents, copy_file, reset_path};
use super::worker::prepare_bare_app;
use super::{BuildPlatform, BuildSummary};

const PACKAGE: &str = "com.appd.runtime";

pub(crate) fn build_android(
    project: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    app_name: &str,
    wrangler: &WranglerConfig,
    plugins: &[Plugin],
) -> Result<BuildSummary> {
    let output = build_dir(project, BuildPlatform::Android);
    let staging = output.join(".appd");
    reset_path(&staging)?;
    let app = staging.join("app");
    let native = app.join("src/main/jniLibs/arm64-v8a");
    let assets = app.join("src/main/assets/app");
    let kotlin = app.join("src/main/kotlin/com/appd/runtime");
    fs::create_dir_all(&native)?;
    fs::create_dir_all(&assets)?;
    fs::create_dir_all(&kotlin)?;

    install_runtime(pack_root, manifest, &native)?;
    prepare_bare_app(&assets, pack_root, manifest, wrangler)?;
    install_shell_sources(pack_root, manifest, &kotlin)?;
    install_plugins(plugins, &app, &kotlin)?;
    write_project(&staging, app_name, plugins)?;

    let status = Command::new("gradle")
        .arg("--no-daemon")
        .arg(":app:assembleDebug")
        .current_dir(&staging)
        .status()
        .context("failed to run Gradle")?;
    if !status.success() {
        bail!("Gradle Android build failed with status {status}");
    }

    let apk = output.join(format!("{app_name}.apk"));
    copy_file(app.join("build/outputs/apk/debug/app-debug.apk"), &apk)?;
    Ok(BuildSummary {
        platform: BuildPlatform::Android,
        bundle_dir: apk,
    })
}

fn install_runtime(pack_root: &Path, manifest: &TargetPackManifest, native: &Path) -> Result<()> {
    let runtime = artifact_path(pack_root, manifest, &ArtifactKind::RuntimeLibrary)?;
    copy_file(runtime, native.join("libappd_shell_android.so"))?;
    let bare = artifact_path(pack_root, manifest, &ArtifactKind::BareRuntimeDirectory)?;
    copy_dir_contents(&bare, native)?;
    Ok(())
}

fn install_shell_sources(
    pack_root: &Path,
    manifest: &TargetPackManifest,
    kotlin: &Path,
) -> Result<()> {
    let source = artifact_path(pack_root, manifest, &ArtifactKind::NativeShellDirectory)?;
    copy_dir_contents(&source, kotlin)
}

fn install_plugins(plugins: &[Plugin], app: &Path, registry_dir: &Path) -> Result<()> {
    let source_dir = app.join("src/main/kotlin/appd/plugins");
    let mut classes = Vec::new();
    for plugin in plugins {
        let Some(platform) = plugin.platform(BuildPlatform::Android) else {
            continue;
        };
        classes.push(platform.class.clone());
        for (index, source) in plugin.sources(BuildPlatform::Android)?.iter().enumerate() {
            let extension = source.extension().and_then(|value| value.to_str());
            if extension != Some("kt") {
                bail!(
                    "Android plugin '{}' source must be Kotlin: {}",
                    plugin.id,
                    source.display()
                );
            }
            let file_name = source
                .file_name()
                .and_then(|value| value.to_str())
                .context("Android plugin source must have a UTF-8 file name")?;
            copy_file(
                source,
                source_dir
                    .join(&plugin.id)
                    .join(format!("{index}-{file_name}")),
            )?;
        }
    }
    fs::write(
        registry_dir.join("AppdPluginRegistry.kt"),
        plugin_registry(&classes),
    )?;
    Ok(())
}

fn plugin_registry(classes: &[String]) -> String {
    let instances = classes
        .iter()
        .map(|class| format!("        {class}(activity),"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "package {PACKAGE}\n\nimport android.app.Activity\n\ninternal fun appdPlugins(activity: Activity): List<AppdPlugin> =\n    listOf(\n{instances}\n    )\n"
    )
}

fn write_project(root: &Path, app_name: &str, plugins: &[Plugin]) -> Result<()> {
    fs::write(
        root.join("settings.gradle"),
        "pluginManagement { repositories { google(); mavenCentral(); gradlePluginPortal() } }\ndependencyResolutionManagement { repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS); repositories { google(); mavenCentral() } }\nrootProject.name = 'appd'\ninclude ':app'\n",
    )?;
    fs::write(
        root.join("build.gradle"),
        "plugins { id 'com.android.application' version '8.7.3' apply false; id 'org.jetbrains.kotlin.android' version '2.1.0' apply false }\n",
    )?;
    fs::write(root.join("gradle.properties"), "android.useAndroidX=true\n")?;
    fs::write(root.join("app/build.gradle"), app_gradle(app_name))?;
    fs::write(
        root.join("app/src/main/AndroidManifest.xml"),
        manifest(app_name, plugins),
    )?;
    Ok(())
}

fn app_gradle(app_name: &str) -> String {
    let application_id = android_application_id(app_name);
    format!(
        "plugins {{ id 'com.android.application'; id 'org.jetbrains.kotlin.android' }}\n\nandroid {{\n  namespace '{PACKAGE}'\n  compileSdk 35\n\n  defaultConfig {{\n    applicationId '{application_id}'\n    minSdk 31\n    targetSdk 35\n    versionCode 1\n    versionName '1.0'\n  }}\n\n  androidResources {{ ignoreAssetsPattern = '' }}\n\n  sourceSets {{ main {{ kotlin.srcDirs += 'src/main/kotlin' }} }}\n\n  compileOptions {{\n    sourceCompatibility JavaVersion.VERSION_17\n    targetCompatibility JavaVersion.VERSION_17\n  }}\n}}\n\nkotlin {{ jvmToolchain(17) }}\n\ndependencies {{\n  implementation 'androidx.webkit:webkit:1.12.1'\n}}\n"
    )
}

fn android_application_id(app_name: &str) -> String {
    let segment = app_name.replace('-', "_");
    let segment = if segment.starts_with(char::is_numeric) {
        format!("app_{segment}")
    } else {
        segment
    };
    format!("com.appd.{segment}")
}

fn manifest(app_name: &str, plugins: &[Plugin]) -> String {
    let permissions = plugins
        .iter()
        .filter_map(|plugin| plugin.platform(BuildPlatform::Android))
        .flat_map(|platform| platform.permissions.iter())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .fold(String::new(), |mut output, permission| {
            output.push_str("  <uses-permission android:name=\"");
            output.push_str(permission);
            output.push_str("\" />\n");
            output
        });
    format!(
        "<manifest xmlns:android=\"http://schemas.android.com/apk/res/android\">\n  <uses-permission android:name=\"android.permission.INTERNET\" />\n{permissions}  <application android:label=\"{app_name}\" android:usesCleartextTraffic=\"false\">\n    <meta-data android:name=\"appd.host\" android:value=\"{app_name}.appd.local\" />\n    <activity android:name=\".AppdActivity\" android:exported=\"true\" android:launchMode=\"singleTask\" android:theme=\"@android:style/Theme.Material.NoActionBar\" android:configChanges=\"colorMode|density|fontScale|keyboard|keyboardHidden|layoutDirection|locale|mcc|mnc|navigation|orientation|screenLayout|screenSize|smallestScreenSize|touchscreen|uiMode\">\n      <intent-filter>\n        <action android:name=\"android.intent.action.MAIN\" />\n        <category android:name=\"android.intent.category.LAUNCHER\" />\n      </intent-filter>\n    </activity>\n  </application>\n</manifest>\n"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn android_project_compiles_the_shell_sources() {
        assert!(super::app_gradle("example").contains("kotlin.srcDirs"));
        assert!(super::app_gradle("example").contains("androidx.webkit"));
    }

    #[test]
    fn android_project_keeps_underscore_assets() {
        assert!(super::app_gradle("example").contains("ignoreAssetsPattern = ''"));
    }

    #[test]
    fn android_project_uses_one_runtime_activity() {
        assert!(super::manifest("example", &[]).contains("android:launchMode=\"singleTask\""));
    }

    #[test]
    fn empty_plugin_registry_is_valid_kotlin() {
        assert!(super::plugin_registry(&[]).contains("listOf("));
    }

    #[test]
    fn android_application_id_accepts_hyphenated_names() {
        assert_eq!(
            super::android_application_id("appd-example-astro"),
            "com.appd.appd_example_astro"
        );
    }
}
