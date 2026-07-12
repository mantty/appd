use std::fs;
use std::path::{Path, PathBuf};

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
    write_manifest_json(
        dir,
        r#"{
  "schemaVersion": 3,
  "appdVersion": "0.1.0",
  "target": "macos-arm64",
  "artifacts": [
    {
      "kind": "runtimeExecutable",
      "path": "bin/appd-runtime",
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    },
    {
      "kind": "runtimeJavaScriptDirectory",
      "path": "runtime-js"
    }
  ],
  "requiredTools": ["node"]
}"#,
    )
}

#[test]
fn lists_supported_targets() -> TestResult {
    let mut cmd = Command::cargo_bin("appd")?;

    cmd.arg("targets")
        .assert()
        .success()
        .stdout(contains("ios-arm64").and(contains("macos-arm64")));

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
    let manifest_path = write_manifest_json(
        temp_dir.path(),
        r#"{
  "schemaVersion": 3,
  "appdVersion": "0.1.0",
  "target": "ios-arm64",
  "artifacts": [
    {
      "kind": "runtimeExecutable",
      "path": "bin/appd-runtime"
    }
  ],
  "requiredTools": []
}"#,
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
  "schemaVersion": 3,
  "appdVersion": "0.1.0",
  "target": "ios-arm64",
  "artifacts": [{"kind": "runtimeExecutable", "path": "../appd-runtime"}],
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
