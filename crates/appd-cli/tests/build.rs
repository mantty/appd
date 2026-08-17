use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use appd_bundle::decompress_worker_bundle;
use appd_quickjs::compile_worker;
use assert_cmd::Command;
use predicates::str::contains;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn create_project(root: &Path) -> TestResult {
    fs::write(
        root.join("package.json"),
        r#"{"name":"demo-app","scripts":{"build":"echo already-built"}}"#,
    )?;
    fs::create_dir_all(root.join("dist/server"))?;
    fs::create_dir_all(root.join("dist/client/styles"))?;
    fs::write(root.join("dist/server/entry.mjs"), "export default {};")?;
    fs::write(root.join("dist/client/index.html"), "<html></html>")?;
    fs::write(root.join("dist/client/styles/app.css"), "body{}")?;
    fs::write(
        root.join("wrangler.jsonc"),
        r#"{
  "main": "dist/server/entry.mjs",
  "assets": { "directory": "dist/client", "binding": "ASSETS" }
}"#,
    )?;
    Ok(())
}

fn install_geolocation_plugin(root: &Path) -> TestResult {
    fs::write(
        root.join("package.json"),
        r#"{"name":"demo-app","scripts":{"build":"echo already-built"},"dependencies":{"@appd/geolocation":"1.0.0"}}"#,
    )?;
    let plugin = root.join("node_modules/@appd/geolocation");
    fs::create_dir_all(plugin.join("apple"))?;
    fs::write(
        plugin.join("apple/GeolocationPlugin.swift"),
        include_str!("../../../plugins/geolocation/apple/GeolocationPlugin.swift"),
    )?;
    fs::write(
        plugin.join("appd-plugin.json"),
        r#"{
  "schemaVersion": 1,
  "id": "geolocation",
  "kind": "frontend",
  "platforms": {
    "macos": {
      "class": "AppdGeolocationPlugin",
      "sources": ["apple/GeolocationPlugin.swift"],
      "frameworks": ["CoreLocation"],
      "plist": {"NSLocationUsageDescription": "Location test"}
    },
    "ios": {
      "class": "AppdGeolocationPlugin",
      "sources": ["apple/GeolocationPlugin.swift"],
      "frameworks": ["CoreLocation"],
      "plist": {"NSLocationWhenInUseUsageDescription": "Location test"}
    },
    "ios-simulator": {
      "class": "AppdGeolocationPlugin",
      "sources": ["apple/GeolocationPlugin.swift"],
      "frameworks": ["CoreLocation"],
      "plist": {"NSLocationWhenInUseUsageDescription": "Location test"}
    }
  }
}"#,
    )?;
    Ok(())
}

fn create_unbuilt_project(root: &Path) -> TestResult {
    fs::write(
        root.join("package.json"),
        r#"{"name":"built-app","scripts":{"build":"node build.cjs"}}"#,
    )?;
    fs::write(
        root.join("build.cjs"),
        r#"const fs = require("node:fs");
fs.mkdirSync("dist/server", { recursive: true });
fs.mkdirSync("dist/client", { recursive: true });
fs.writeFileSync("dist/server/entry.mjs", "export default {};");
fs.writeFileSync("dist/client/index.html", "<html></html>");
fs.writeFileSync("dist/server/wrangler.json", JSON.stringify({
  main: "entry.mjs",
  assets: { directory: "../client", binding: "ASSETS" }
}));
"#,
    )?;
    Ok(())
}

fn create_target_pack(root: &Path, target: &str) -> TestResult<PathBuf> {
    let framework = root.join("frameworks/AppdRuntime.framework");
    let shell = root.join("native-shell");
    fs::create_dir_all(&framework)?;
    fs::create_dir_all(&shell)?;
    fs::create_dir_all(root.join("tools/runtime-js"))?;
    create_test_framework(root, target)?;
    write_test_shell(&shell)?;
    fs::write(
        root.join("tools/esbuild.cjs"),
        "#!/usr/bin/env node\nconst fs = require('node:fs');\nconst args = process.argv.slice(2);\nconst output = args.find((arg) => arg.startsWith('--outfile=')).slice('--outfile='.length);\nconst input = args.at(-1);\nfs.copyFileSync(input, output);\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join("tools/esbuild.cjs");
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    fs::write(
        root.join("target-pack.json"),
        format!(
            r#"{{
  "schemaVersion": 13,
  "appdVersion": "0.1.0",
  "target": "{target}",
  "artifacts": [
    {{"kind": "runtimeLibrary", "path": "frameworks/AppdRuntime.framework"}},
    {{"kind": "nativeShellDirectory", "path": "native-shell"}},
    {{"kind": "runtimeJavaScriptDirectory", "path": "tools/runtime-js"}},
    {{"kind": "esbuildExecutable", "path": "tools/esbuild.cjs"}}
  ],
  "requiredTools": ["node", "xcrun"]
}}"#
        ),
    )?;
    Ok(root.join("target-pack.json"))
}

fn write_test_shell(shell: &Path) -> TestResult {
    fs::write(
        shell.join("AppdShell.swift"),
        r#"import Foundation

struct AppdPluginError: Error {
  let name: String
  let message: String

  static func notSupported(_ message: String) -> Self {
    Self(name: "NotSupportedError", message: message)
  }
}

typealias AppdPluginReply = (Result<Any?, AppdPluginError>) -> Void

protocol AppdPlugin: AnyObject {
  var id: String { get }
  func call(method: String, arguments: Any, reply: @escaping AppdPluginReply)
  func subscribe(
    method: String,
    arguments: Any,
    reply: @escaping AppdPluginReply
  ) -> (() -> Void)
}

@main struct App { static func main() {} }
"#,
    )?;
    Ok(())
}

fn create_test_framework(root: &Path, target: &str) -> TestResult {
    let source = root.join("runtime.c");
    let object = root.join("runtime.o");
    fs::write(&source, "void appd_test(void) {}")?;

    let (sdk, triple) = match target {
        "macos-arm64" => ("macosx", "arm64-apple-macos14.0"),
        "macos-x64" => ("macosx", "x86_64-apple-macos14.0"),
        "ios-arm64" => ("iphoneos", "arm64-apple-ios17.0"),
        "ios-simulator-arm64" => ("iphonesimulator", "arm64-apple-ios17.0-simulator"),
        "ios-simulator-x64" => ("iphonesimulator", "x86_64-apple-ios17.0-simulator"),
        _ => return Err(format!("unsupported Apple test target: {target}").into()),
    };
    let status = ProcessCommand::new("xcrun")
        .args(["--sdk", sdk, "clang", "-target", triple, "-c"])
        .arg(&source)
        .args(["-o"])
        .arg(&object)
        .status()?;
    if !status.success() {
        return Err(format!("test framework compilation failed with {status}").into());
    }
    let status = ProcessCommand::new("xcrun")
        .args(["libtool", "-static", "-o"])
        .arg(root.join("frameworks/AppdRuntime.framework/AppdRuntime"))
        .arg(&object)
        .status()?;
    if !status.success() {
        return Err(format!("test framework archive failed with {status}").into());
    }
    fs::remove_file(source)?;
    fs::remove_file(object)?;
    Ok(())
}

fn create_inputs(target: &str) -> TestResult<(tempfile::TempDir, PathBuf, PathBuf)> {
    let temporary = tempfile::tempdir()?;
    let project = temporary.path().join("project");
    let pack = temporary.path().join("pack");
    fs::create_dir_all(&project)?;
    fs::create_dir_all(&pack)?;
    create_project(&project)?;
    let manifest = create_target_pack(&pack, target)?;
    Ok((temporary, project, manifest))
}

fn create_windows_inputs() -> TestResult<(tempfile::TempDir, PathBuf, PathBuf)> {
    let temporary = tempfile::tempdir()?;
    let project = temporary.path().join("project");
    let pack = temporary.path().join("pack");
    fs::create_dir_all(&project)?;
    fs::create_dir_all(pack.join("bin"))?;
    fs::create_dir_all(pack.join("tools/runtime-js"))?;
    create_project(&project)?;
    fs::write(pack.join("bin/appd-shell-windows.exe"), "shell")?;
    fs::write(
        pack.join("tools/esbuild.cjs"),
        "#!/usr/bin/env node\nconst fs = require('node:fs');\nconst args = process.argv.slice(2);\nconst output = args.find((arg) => arg.startsWith('--outfile=')).slice('--outfile='.length);\nconst input = args.at(-1);\nfs.copyFileSync(input, output);\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = pack.join("tools/esbuild.cjs");
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    fs::write(
        pack.join("target-pack.json"),
        r#"{
  "schemaVersion": 13,
  "appdVersion": "0.1.0",
  "target": "windows-x64",
  "artifacts": [
    {"kind": "runtimeExecutable", "path": "bin/appd-shell-windows.exe"},
    {"kind": "runtimeJavaScriptDirectory", "path": "tools/runtime-js"},
    {"kind": "esbuildExecutable", "path": "tools/esbuild.cjs"}
  ],
  "requiredTools": ["node"]
}"#,
    )?;
    Ok((temporary, project, pack.join("target-pack.json")))
}

fn build_command(platform: &str, project: &Path, manifest: &Path) -> TestResult<Command> {
    let mut command = Command::cargo_bin("appd")?;
    command
        .args(["build", platform, "--project"])
        .arg(project)
        .arg("--target-pack")
        .arg(manifest)
        .arg("--skip-web-build");
    Ok(command)
}

#[test]
fn builds_macos_app_with_quickjs_bundle_and_assets() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
    build_command("macos", &project, &manifest)?
        .assert()
        .success()
        .stdout(contains("Built macOS bundle"));

    let bundle = project.join("build/macos/demo-app.app");
    let app = bundle.join("Contents/Resources/app");
    assert!(bundle.join("Contents/MacOS/demo-app").is_file());
    assert!(
        !bundle
            .join("Contents/Frameworks/AppdRuntime.framework")
            .exists()
    );
    assert_eq!(
        decompress_worker_bundle(&fs::read(app.join("worker.bundle"))?)?,
        compile_worker(&fs::read(project.join("dist/server/entry.mjs"))?)?
    );
    assert!(app.join("assets/index.html").is_file());
    let plist = fs::read_to_string(bundle.join("Contents/Info.plist"))?;
    assert!(plist.contains("NSAllowsLocalNetworking"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(app.join("asset-manifest.json"))?)?;
    assert_eq!(manifest["files"]["styles/app.css"], "text/css");
    assert!(!app.join("config.capnp").exists());
    Ok(())
}

#[test]
fn compiles_a_self_contained_worker() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
    fs::write(
        project.join("dist/server/entry.mjs"),
        "export default { fetch() { return new Response('ok'); } };",
    )?;

    build_command("macos", &project, &manifest)?
        .assert()
        .success();

    let bundle = project.join("build/macos/demo-app.app/Contents/Resources/app");
    assert_eq!(
        decompress_worker_bundle(&fs::read(bundle.join("worker.bundle"))?)?,
        compile_worker(&fs::read(project.join("dist/server/entry.mjs"))?)?
    );
    Ok(())
}

#[test]
fn packages_worker_vars_as_a_normalized_manifest() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
    fs::write(
        project.join("wrangler.jsonc"),
        r#"{
  "main": "dist/server/entry.mjs",
  "vars": { "TEXT": "value", "JSON": { "enabled": true } }
}"#,
    )?;

    build_command("macos", &project, &manifest)?
        .assert()
        .success();

    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(
        project.join("build/macos/demo-app.app/Contents/Resources/app/worker-environment.json"),
    )?)?;
    assert_eq!(manifest["vars"]["TEXT"], "value");
    assert_eq!(
        manifest["vars"]["JSON"],
        serde_json::json!({ "enabled": true })
    );
    Ok(())
}

#[test]
fn builds_declared_plugins_into_the_macos_shell() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
    install_geolocation_plugin(&project)?;

    build_command("macos", &project, &manifest)?
        .assert()
        .success();

    let plist = fs::read_to_string(project.join("build/macos/demo-app.app/Contents/Info.plist"))?;
    assert!(plist.contains("<key>NSLocationUsageDescription</key><string>Location test</string>"));
    Ok(())
}

#[test]
fn builds_intel_macos_app() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-x64")?;
    build_command("macos", &project, &manifest)?
        .assert()
        .success();

    assert!(project.join("build/macos/demo-app.app").is_dir());
    Ok(())
}

#[test]
fn builds_windows_app_with_its_runtime_files() -> TestResult {
    let (_temporary, project, manifest) = create_windows_inputs()?;
    build_command("windows", &project, &manifest)?
        .assert()
        .success()
        .stdout(contains("Built Windows bundle"));

    let bundle = project.join("build/windows/demo-app");
    assert!(bundle.join("demo-app.exe").is_file());
    assert!(!bundle.join("appd-runtime.dll").exists());
    assert!(bundle.join("app/worker.bundle").is_file());
    let config: serde_json::Value = serde_json::from_slice(&fs::read(bundle.join("appd.json"))?)?;
    assert_eq!(config["host"], "demo-app.appd.local");
    Ok(())
}

#[test]
fn requires_a_target_pack_for_app_builds() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let project = temporary.path().join("project");
    fs::create_dir_all(&project)?;
    create_project(&project)?;

    let mut command = Command::cargo_bin("appd")?;
    command
        .args(["build", "macos", "--project"])
        .arg(&project)
        .arg("--skip-web-build")
        .assert()
        .failure()
        .stderr(contains("no target pack found"));
    Ok(())
}

#[test]
fn builds_web_project_before_loading_generated_config() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let project = temporary.path().join("project");
    let pack = temporary.path().join("pack");
    fs::create_dir_all(&project)?;
    fs::create_dir_all(&pack)?;
    create_unbuilt_project(&project)?;
    let manifest = create_target_pack(&pack, "macos-arm64")?;
    let config = project.join("dist/server/wrangler.json");

    let mut command = Command::cargo_bin("appd")?;
    command
        .args(["build", "macos", "--project"])
        .arg(&project)
        .arg("--target-pack")
        .arg(manifest)
        .arg("--config")
        .arg(config);
    command.assert().success();

    assert!(project.join("build/macos/built-app.app").is_dir());
    Ok(())
}

#[test]
fn builds_physical_ios_app() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("ios-arm64")?;
    install_geolocation_plugin(&project)?;
    build_command("ios", &project, &manifest)?
        .assert()
        .success()
        .stdout(contains("Built iOS bundle"));

    let bundle = project.join("build/ios/demo-app.app");
    assert!(bundle.join("demo-app").is_file());
    assert!(bundle.join("app/worker.bundle").is_file());
    assert!(!bundle.join("Frameworks/AppdRuntime.framework").exists());
    let plist = fs::read_to_string(bundle.join("Info.plist"))?;
    assert!(plist.contains("LSRequiresIPhoneOS"));
    assert!(plist.contains("UIDeviceFamily"));
    assert!(plist.contains("UILaunchScreen"));
    assert!(plist.contains("NSAllowsLocalNetworking"));
    assert!(plist.contains("NSLocationWhenInUseUsageDescription"));
    Ok(())
}

#[test]
fn builds_ios_simulator_app() -> TestResult {
    for target in ["ios-simulator-arm64", "ios-simulator-x64"] {
        let (_temporary, project, manifest) = create_inputs(target)?;
        install_geolocation_plugin(&project)?;
        build_command("ios-simulator", &project, &manifest)?
            .assert()
            .success()
            .stdout(contains("Built iOS Simulator bundle"));

        let bundle = project.join("build/ios-simulator/demo-app.app");
        assert!(bundle.join("demo-app").is_file());
        assert!(bundle.join("app/worker.bundle").is_file());
        assert!(!bundle.join("Frameworks/AppdRuntime.framework").exists());
        assert!(!bundle.join("embedded.mobileprovision").exists());
        let plist = fs::read_to_string(bundle.join("Info.plist"))?;
        assert!(plist.contains("iPhoneSimulator"));
        assert!(plist.contains("NSLocationWhenInUseUsageDescription"));
    }
    Ok(())
}

#[test]
fn writes_configured_asset_routing_modes() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
    fs::write(
        project.join("wrangler.jsonc"),
        r#"{
  "main": "dist/server/entry.mjs",
  "assets": {
    "directory": "dist/client",
    "html_handling": "drop-trailing-slash",
    "not_found_handling": "single-page-application"
  }
}"#,
    )?;
    build_command("macos", &project, &manifest)?
        .assert()
        .success();

    let path = project.join("build/macos/demo-app.app/Contents/Resources/app/asset-manifest.json");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    assert_eq!(value["htmlHandling"], "drop-trailing-slash");
    assert_eq!(value["notFoundHandling"], "single-page-application");
    Ok(())
}

#[test]
fn rejects_webassembly_modules() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
    fs::write(project.join("dist/server/module.wasm"), b"wasm")?;

    build_command("macos", &project, &manifest)?
        .assert()
        .failure()
        .stderr(contains("WebAssembly files are not supported"));
    Ok(())
}

#[test]
fn rejects_webassembly_assets() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
    fs::write(project.join("dist/client/module.wasm"), b"wasm")?;

    build_command("macos", &project, &manifest)?
        .assert()
        .failure()
        .stderr(contains("WebAssembly files are not supported"));
    Ok(())
}

#[test]
fn rejects_unsafe_package_names() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
    fs::write(project.join("package.json"), r#"{"name":"../demo"}"#)?;

    build_command("macos", &project, &manifest)?
        .assert()
        .failure()
        .stderr(contains("package.json name is not a safe app name"));
    Ok(())
}

#[test]
fn rejects_package_names_that_are_not_dns_labels() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
    fs::write(project.join("package.json"), r#"{"name":"Demo_App"}"#)?;

    build_command("macos", &project, &manifest)?
        .assert()
        .failure()
        .stderr(contains("package.json name is not a safe app name"));
    Ok(())
}

#[test]
fn requires_a_package_name() -> TestResult {
    for package in ["{}", r#"{"name":""}"#] {
        let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
        fs::write(project.join("package.json"), package)?;

        build_command("macos", &project, &manifest)?
            .assert()
            .failure()
            .stderr(contains("package.json name is required"));
    }
    Ok(())
}

#[test]
fn rejects_package_names_outside_dns_label_bounds() -> TestResult {
    let too_long = "a".repeat(64);
    for name in ["-demo", "demo-", &too_long] {
        let (_temporary, project, manifest) = create_inputs("macos-arm64")?;
        fs::write(
            project.join("package.json"),
            format!(r#"{{"name":"{name}"}}"#),
        )?;

        build_command("macos", &project, &manifest)?
            .assert()
            .failure()
            .stderr(contains("package.json name is not a safe app name"));
    }
    Ok(())
}

#[test]
fn rejects_target_pack_for_another_platform() -> TestResult {
    let (_temporary, project, manifest) = create_inputs("ios-arm64")?;
    build_command("macos", &project, &manifest)?
        .assert()
        .failure()
        .stderr(contains("target pack ios-arm64 cannot build macOS"));
    Ok(())
}
