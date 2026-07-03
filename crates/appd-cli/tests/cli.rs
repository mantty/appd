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
  "schemaVersion": 1,
  "appdVersion": "0.1.0",
  "target": "android-arm64",
  "artifacts": [
    {
      "kind": "runtimeSharedLibrary",
      "path": "lib/libappd.so",
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    },
    {
      "kind": "androidManifest",
      "path": "AndroidManifest.xml"
    }
  ],
  "requiredTools": ["android-sdk"]
}"#,
    )
}

#[test]
fn lists_supported_targets() -> TestResult {
    let mut cmd = Command::cargo_bin("appd")?;

    cmd.arg("targets").assert().success().stdout(
        contains("ios-simulator-arm64")
            .and(contains("android-arm64"))
            .and(contains("windows-x64")),
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
        .stdout(contains("target: android-arm64"))
        .stdout(contains("artifacts: 2"))
        .stdout(contains("required tools: android-sdk"));

    Ok(())
}

#[test]
fn inspects_target_pack_manifest_without_required_tools() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let manifest_path = write_manifest_json(
        temp_dir.path(),
        r#"{
  "schemaVersion": 1,
  "appdVersion": "0.1.0",
  "target": "linux-x64",
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
        .stdout(contains("target: linux-x64"))
        .stdout(contains("required tools: none"));

    Ok(())
}

#[test]
fn rejects_invalid_target_pack_manifest() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let manifest_path = write_manifest_json(
        temp_dir.path(),
        r#"{
  "schemaVersion": 1,
  "appdVersion": "0.1.0",
  "target": "android-arm64",
  "artifacts": [{"kind": "runtimeSharedLibrary", "path": "../libappd.so"}],
  "requiredTools": []
}"#,
    )?;
    let mut cmd = Command::cargo_bin("appd")?;

    cmd.args(["pack", "inspect"])
        .arg(&manifest_path)
        .assert()
        .failure()
        .stderr(contains("invalid target pack").and(contains(
            "artifact path must stay inside the target pack: ../libappd.so",
        )));

    Ok(())
}
