use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::str::contains;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn create_project(root: &Path) -> TestResult {
    fs::write(
        root.join("package.json"),
        r#"{"name":"demo-app","scripts":{"build":"echo already-built"}}"#,
    )?;
    fs::create_dir_all(root.join("dist/server"))?;
    fs::create_dir_all(root.join("dist/client/_astro"))?;
    fs::write(root.join("dist/server/entry.mjs"), "export default {};")?;
    fs::write(root.join("dist/client/index.html"), "<html></html>")?;
    fs::write(
        root.join("dist/client/_astro/app.js"),
        "console.log('app');",
    )?;
    fs::write(
        root.join("wrangler.jsonc"),
        r#"{
  "main": "dist/server/entry.mjs",
  "compatibility_date": "2024-09-23",
  "compatibility_flags": ["nodejs_compat"],
  "assets": {
    "directory": "dist/client",
    "binding": "ASSETS"
  }
}"#,
    )?;
    Ok(())
}

fn write_target_pack(root: &Path, target: &str, artifacts: &[(&str, &str)]) -> TestResult<PathBuf> {
    let artifacts = artifacts
        .iter()
        .map(|(kind, path)| format!(r#"    {{"kind": "{kind}", "path": "{path}"}}"#))
        .collect::<Vec<_>>()
        .join(",\n");
    fs::write(
        root.join("target-pack.json"),
        format!(
            r#"{{
  "schemaVersion": 1,
  "appdVersion": "0.1.0",
  "target": "{target}",
  "artifacts": [
{artifacts}
  ],
  "requiredTools": []
}}"#
        ),
    )?;
    Ok(root.join("target-pack.json"))
}

fn create_runtime_executable_target_pack(
    root: &Path,
    target: &str,
    binary_name: &str,
) -> TestResult<PathBuf> {
    fs::create_dir_all(root.join("bin"))?;
    let binary_path = format!("bin/{binary_name}");
    fs::write(root.join(&binary_path), "runtime")?;
    write_target_pack(root, target, &[("runtimeExecutable", binary_path.as_str())])
}

fn create_linux_target_pack(root: &Path) -> TestResult<PathBuf> {
    create_runtime_executable_target_pack(root, "linux-x64", "appd-runtime")
}

fn create_windows_target_pack(root: &Path) -> TestResult<PathBuf> {
    create_runtime_executable_target_pack(root, "windows-x64", "appd-runtime.exe")
}

fn create_macos_target_pack(root: &Path) -> TestResult<PathBuf> {
    create_runtime_executable_target_pack(root, "macos-arm64", "appd-runtime")
}

fn create_ios_target_pack(root: &Path) -> TestResult<PathBuf> {
    create_runtime_executable_target_pack(root, "ios-simulator-arm64", "appd-runtime")
}

fn create_android_target_pack(root: &Path) -> TestResult<PathBuf> {
    fs::create_dir_all(root.join("lib"))?;
    fs::create_dir_all(root.join("java"))?;
    fs::create_dir_all(root.join("res/xml"))?;
    fs::write(root.join("lib/libappd.so"), "runtime")?;
    fs::write(root.join("java/classes.dex"), "dex")?;
    fs::write(
        root.join("AndroidManifest.xml.in"),
        r#"<manifest package="__PACKAGE__"><application android:label="__APP_NAME__"/></manifest>"#,
    )?;
    fs::write(
        root.join("res/xml/network_security_config.xml"),
        "<config/>",
    )?;
    write_target_pack(
        root,
        "android-arm64",
        &[
            ("runtimeSharedLibrary", "lib/libappd.so"),
            ("androidDex", "java/classes.dex"),
            ("androidManifest", "AndroidManifest.xml.in"),
            ("resourceDirectory", "res"),
        ],
    )
}

fn create_auto_target_pack_root(root: &Path) -> TestResult<PathBuf> {
    let pack_root = root.join("target-packs");
    create_macos_target_pack(&pack_root.join("macos-arm64"))?;
    create_ios_target_pack(&pack_root.join("ios-simulator-arm64"))?;
    Ok(pack_root)
}

fn create_auto_target_build_inputs() -> TestResult<(tempfile::TempDir, PathBuf, PathBuf)> {
    let temp_dir = tempfile::tempdir()?;
    let project = temp_dir.path().join("project");
    fs::create_dir_all(&project)?;
    create_project(&project)?;
    let pack_root = create_auto_target_pack_root(temp_dir.path())?;
    Ok((temp_dir, project, pack_root))
}

fn create_build_inputs(
    create_target_pack: fn(&Path) -> TestResult<PathBuf>,
) -> TestResult<(tempfile::TempDir, PathBuf, PathBuf)> {
    let temp_dir = tempfile::tempdir()?;
    let project = temp_dir.path().join("project");
    let pack = temp_dir.path().join("pack");
    fs::create_dir_all(&project)?;
    fs::create_dir_all(&pack)?;
    create_project(&project)?;
    let manifest = create_target_pack(&pack)?;
    Ok((temp_dir, project, manifest))
}

fn assert_workerd_app(app_dir: &Path) {
    assert!(app_dir.join("config.capnp").is_file());
    assert!(app_dir.join("assets-worker.mjs").is_file());
    assert!(!app_dir.join("kv-asset-handler.js").exists());
    assert!(app_dir.join("worker/entry.mjs").is_file());
    assert!(app_dir.join("assets/index.html").is_file());
}

fn assert_jitless(app_dir: &Path, expected: bool) -> TestResult {
    let config = fs::read_to_string(app_dir.join("config.capnp"))?;
    assert_eq!(config.contains(r"--jitless"), expected);
    Ok(())
}

fn build_command(platform: &str, project: &Path, manifest: &Path) -> TestResult<Command> {
    let mut cmd = Command::cargo_bin("appd")?;
    cmd.args(["build", platform])
        .arg("--project")
        .arg(project)
        .arg("--target-pack")
        .arg(manifest)
        .arg("--skip-web-build");
    Ok(cmd)
}

fn build_platforms_command(
    platforms: &str,
    project: &Path,
    pack_root: &Path,
) -> TestResult<Command> {
    let mut cmd = Command::cargo_bin("appd")?;
    cmd.args(["build", "--platforms", platforms])
        .arg("--project")
        .arg(project)
        .arg("--skip-web-build")
        .env("appd_target_pack_dir", pack_root);
    Ok(cmd)
}

#[test]
fn builds_linux_bundle_from_existing_dist_and_target_pack() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_linux_target_pack)?;

    build_command("linux", &project, &manifest)?
        .assert()
        .success()
        .stdout(contains("Built Linux bundle"));

    let bundle = project.join("build/linux/demo-app");
    assert!(bundle.join("demo-app").is_file());
    assert_workerd_app(&bundle.join("app"));
    assert_jitless(&bundle.join("app"), false)?;

    Ok(())
}

#[test]
fn builds_windows_bundle_from_existing_dist_and_target_pack() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_windows_target_pack)?;

    build_command("windows", &project, &manifest)?
        .assert()
        .success()
        .stdout(contains("Built Windows bundle"));

    let bundle = project.join("build/windows/demo-app");
    assert!(bundle.join("demo-app.exe").is_file());
    assert_workerd_app(&bundle.join("app"));
    assert_jitless(&bundle.join("app"), false)?;

    Ok(())
}

#[test]
fn builds_macos_app_bundle_from_existing_dist_and_target_pack() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_macos_target_pack)?;

    build_command("macos", &project, &manifest)?
        .assert()
        .success()
        .stdout(contains("Built macOS bundle"));

    let bundle = project.join("build/macos/demo-app.app");
    assert!(bundle.join("Contents/MacOS/demo-app").is_file());
    assert!(bundle.join("Contents/Info.plist").is_file());
    assert_workerd_app(&bundle.join("Contents/Resources/app"));
    assert_jitless(&bundle.join("Contents/Resources/app"), false)?;

    Ok(())
}

#[test]
fn builds_bundle_from_explicit_wrangler_config_path() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_macos_target_pack)?;
    fs::create_dir_all(project.join("custom/server"))?;
    fs::create_dir_all(project.join("custom/client"))?;
    fs::write(
        project.join("custom/server/entry.mjs"),
        "export const source = 'custom-worker';",
    )?;
    fs::write(
        project.join("custom/client/index.html"),
        "<html>custom</html>",
    )?;
    fs::create_dir_all(project.join("config"))?;
    let config_path = project.join("config/custom.toml");
    fs::write(
        &config_path,
        r#"
main = "../custom/server/entry.mjs"

[assets]
directory = "../custom/client"
binding = "STATIC"
html_handling = "drop-trailing-slash"
not_found_handling = "single-page-application"
"#,
    )?;

    build_command("macos", &project, &manifest)?
        .arg("--config")
        .arg(&config_path)
        .assert()
        .success()
        .stdout(contains("Built macOS bundle"));

    let app_dir = project.join("build/macos/demo-app.app/Contents/Resources/app");
    assert_eq!(
        fs::read_to_string(app_dir.join("worker/entry.mjs"))?,
        "export const source = 'custom-worker';"
    );
    assert_eq!(
        fs::read_to_string(app_dir.join("assets/index.html"))?,
        "<html>custom</html>"
    );
    let config_capnp = fs::read_to_string(app_dir.join("config.capnp"))?;
    assert!(config_capnp.contains(r#"(name = "STATIC", service = "assets")"#));
    assert!(config_capnp.contains(r#"\"htmlHandling\":\"drop-trailing-slash\""#));
    assert!(config_capnp.contains(r#"\"notFoundHandling\":\"single-page-application\""#));

    Ok(())
}

#[test]
fn builds_bundle_from_short_config_flag() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_linux_target_pack)?;

    build_command("linux", &project, &manifest)?
        .arg("-c")
        .arg(project.join("wrangler.jsonc"))
        .assert()
        .success()
        .stdout(contains("Built Linux bundle"));

    Ok(())
}

#[test]
fn builds_ios_app_bundle_with_jitless_config() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_ios_target_pack)?;

    build_command("ios", &project, &manifest)?
        .assert()
        .success()
        .stdout(contains("Built iOS bundle"));

    let bundle = project.join("build/ios/demo-app.app");
    assert!(bundle.join("demo-app").is_file());
    assert!(bundle.join("Info.plist").is_file());
    assert_workerd_app(&bundle.join("app"));
    assert_jitless(&bundle.join("app"), true)?;

    Ok(())
}

#[test]
fn builds_android_stage_with_jitless_config_and_manifest_substitution() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_android_target_pack)?;

    build_command("android", &project, &manifest)?
        .assert()
        .success()
        .stdout(contains("Built Android bundle"));

    let stage = project.join("build/android/stage");
    assert!(stage.join("lib/arm64-v8a/libappd.so").is_file());
    assert!(stage.join("classes.dex").is_file());
    assert!(stage.join("xml/network_security_config.xml").is_file());
    let android_manifest = fs::read_to_string(stage.join("AndroidManifest.xml"))?;
    assert!(android_manifest.contains(r#"package="com.appd.demo_app""#));
    assert!(android_manifest.contains(r#"android:label="demo-app""#));
    assert_workerd_app(&stage.join("assets/app"));
    assert_jitless(&stage.join("assets/app"), true)?;

    Ok(())
}

#[test]
fn rejects_build_when_server_entry_is_missing() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_linux_target_pack)?;
    fs::remove_file(project.join("dist/server/entry.mjs"))?;

    build_command("linux", &project, &manifest)?
        .assert()
        .failure()
        .stderr(contains("worker main not found"));

    Ok(())
}

#[test]
fn rejects_build_when_assets_directory_is_missing() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_linux_target_pack)?;
    fs::remove_dir_all(project.join("dist/client"))?;

    build_command("linux", &project, &manifest)?
        .assert()
        .failure()
        .stderr(contains("assets directory not found"));

    Ok(())
}

#[test]
fn rejects_build_when_target_pack_does_not_match_platform() -> TestResult {
    let (_temp_dir, project, manifest) = create_build_inputs(create_android_target_pack)?;

    build_command("linux", &project, &manifest)?
        .assert()
        .failure()
        .stderr(contains("target pack android-arm64 cannot build Linux"));

    Ok(())
}

#[test]
fn builds_macos_bundle_from_platforms_flag_and_discovered_target_pack() -> TestResult {
    let (_temp_dir, project, pack_root) = create_auto_target_build_inputs()?;

    build_platforms_command("macos", &project, &pack_root)?
        .assert()
        .success()
        .stdout(contains("Built macOS bundle"));

    let bundle = project.join("build/macos/demo-app.app");
    assert!(bundle.join("Contents/MacOS/demo-app").is_file());
    assert_workerd_app(&bundle.join("Contents/Resources/app"));

    Ok(())
}

#[test]
fn builds_ios_simulator_bundle_from_platforms_flag_and_discovered_target_pack() -> TestResult {
    let (_temp_dir, project, pack_root) = create_auto_target_build_inputs()?;

    build_platforms_command("ios-simulator", &project, &pack_root)?
        .assert()
        .success()
        .stdout(contains("Built iOS Simulator bundle"));

    let bundle = project.join("build/ios-simulator/demo-app.app");
    assert!(bundle.join("demo-app").is_file());
    assert!(bundle.join("Info.plist").is_file());
    assert_workerd_app(&bundle.join("app"));
    assert_jitless(&bundle.join("app"), true)?;

    Ok(())
}

#[test]
fn builds_multiple_platforms_without_output_collisions() -> TestResult {
    let (_temp_dir, project, pack_root) = create_auto_target_build_inputs()?;

    build_platforms_command("macos,ios-simulator", &project, &pack_root)?
        .assert()
        .success()
        .stdout(contains("Built macOS bundle"))
        .stdout(contains("Built iOS Simulator bundle"));

    assert!(project.join("build/macos/demo-app.app").is_dir());
    assert!(project.join("build/ios-simulator/demo-app.app").is_dir());

    Ok(())
}
