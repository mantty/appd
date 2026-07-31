use std::fs;
use std::path::{Path, PathBuf};

use appd_target_pack::artifact_sha256;
use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn write_manifest_json(dir: &Path, json: &str) -> TestResult<PathBuf> {
    let manifest_path = dir.join("target-pack.json");
    fs::write(&manifest_path, json)?;
    Ok(manifest_path)
}

fn write_manifest(dir: &Path) -> TestResult<PathBuf> {
    fs::create_dir_all(dir.join("bin"))?;
    fs::create_dir_all(dir.join("runtime-js"))?;
    fs::write(dir.join("bin/appd-runtime"), "runtime")?;
    fs::write(dir.join("runtime-js/bootstrap.js"), "bootstrap")?;
    let runtime_hash = artifact_sha256(dir.join("bin/appd-runtime"))?;
    let runtime_js_hash = artifact_sha256(dir.join("runtime-js"))?;
    write_manifest_json(
        dir,
        &format!(
            r#"{{
  "schemaVersion": 7,
  "appdVersion": "0.1.0",
  "target": "macos-arm64",
  "artifacts": [
    {{
      "kind": "runtimeLibrary",
      "path": "bin/appd-runtime",
      "sha256": "{runtime_hash}"
    }},
    {{
      "kind": "runtimeJavaScriptDirectory",
      "path": "runtime-js",
      "sha256": "{runtime_js_hash}"
    }}
  ],
  "requiredTools": ["node"]
}}"#
        ),
    )
}

#[test]
fn lists_supported_targets() -> TestResult {
    let mut cmd = Command::cargo_bin("appd")?;

    cmd.arg("targets").assert().success().stdout(
        contains("android-arm64")
            .and(contains("ios-arm64"))
            .and(contains("ios-simulator-arm64"))
            .and(contains("ios-simulator-x64"))
            .and(contains("macos-arm64"))
            .and(contains("macos-x64")),
    );

    Ok(())
}

#[test]
fn inspects_target_pack_manifest() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let manifest_path = write_manifest(temp_dir.path())?;
    let mut cmd = Command::cargo_bin("appd")?;

    cmd.args(["pack", "inspect"])
        .arg(&manifest_path)
        .assert()
        .success()
        .stdout(contains("target: macos-arm64"))
        .stdout(contains("artifacts: 2"))
        .stdout(contains("required tools: node"));

    Ok(())
}

#[test]
fn inspects_target_pack_manifest_without_required_tools() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    fs::create_dir_all(temp_dir.path().join("bin"))?;
    fs::write(temp_dir.path().join("bin/appd-runtime"), "runtime")?;
    let runtime_hash = artifact_sha256(temp_dir.path().join("bin/appd-runtime"))?;
    let manifest_path = write_manifest_json(
        temp_dir.path(),
        &format!(
            r#"{{
  "schemaVersion": 7,
  "appdVersion": "0.1.0",
  "target": "ios-arm64",
  "artifacts": [
    {{
      "kind": "runtimeLibrary",
      "path": "bin/appd-runtime",
      "sha256": "{runtime_hash}"
    }}
  ],
  "requiredTools": []
}}"#
        ),
    )?;
    let mut cmd = Command::cargo_bin("appd")?;

    cmd.args(["pack", "inspect"])
        .arg(&manifest_path)
        .assert()
        .success()
        .stdout(contains("target: ios-arm64"))
        .stdout(contains("required tools: none"));

    Ok(())
}

#[test]
fn rejects_invalid_target_pack_manifest() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let manifest_path = write_manifest_json(
        temp_dir.path(),
        r#"{
  "schemaVersion": 7,
  "appdVersion": "0.1.0",
  "target": "ios-arm64",
  "artifacts": [{"kind": "runtimeLibrary", "path": "../appd-runtime", "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}],
  "requiredTools": []
}"#,
    )?;
    let mut cmd = Command::cargo_bin("appd")?;

    cmd.args(["pack", "inspect"])
        .arg(&manifest_path)
        .assert()
        .failure()
        .stderr(contains("invalid target pack").and(contains(
            "artifact path must stay inside the target pack: ../appd-runtime",
        )));

    Ok(())
}
