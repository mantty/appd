use std::fs;
use std::str::FromStr;

use appd_target_pack::{
    Artifact, ArtifactKind, Target, TargetPackError, TargetPackManifest, TargetPackVersion,
    load_manifest, write_manifest,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn valid_manifest() -> TargetPackManifest {
    TargetPackManifest {
        schema_version: TargetPackVersion::CURRENT,
        appd_version: "0.1.0".to_owned(),
        target: Target::IosArm64,
        artifacts: vec![
            Artifact {
                kind: ArtifactKind::RuntimeLibrary,
                path: "frameworks/AppdRuntime.framework".to_owned(),
            },
            Artifact {
                kind: ArtifactKind::RuntimeJavaScriptDirectory,
                path: "runtime-js".to_owned(),
            },
        ],
        required_tools: vec!["xcode".to_owned()],
    }
}

fn assert_unsafe_artifact_path(path: &str) {
    let mut manifest = valid_manifest();
    path.clone_into(&mut manifest.artifacts[0].path);

    assert!(matches!(
        manifest.validate(),
        Err(TargetPackError::UnsafeArtifactPath(rejected)) if rejected == path
    ));
}

#[test]
fn parses_known_targets_and_rejects_unknown_targets() {
    let parsed = Target::from_str("ios-arm64").map_err(|error| error.to_string());
    assert_eq!(parsed, Ok(Target::IosArm64));
    let simulator = Target::from_str("ios-simulator-arm64").map_err(|error| error.to_string());
    assert_eq!(simulator, Ok(Target::IosSimulatorArm64));
    assert_eq!(Target::IosSimulatorX64.to_string(), "ios-simulator-x64");
    assert_eq!(Target::MacosArm64.to_string(), "macos-arm64");
    assert_eq!(Target::MacosX64.to_string(), "macos-x64");
    assert_eq!(Target::WindowsX64.to_string(), "windows-x64");
    assert!(matches!(
        Target::from_str("ios-armv7"),
        Err(TargetPackError::UnknownTarget(target)) if target == "ios-armv7"
    ));
}

#[test]
fn validates_manifest_with_relative_artifact_paths() {
    let manifest = valid_manifest();

    assert!(manifest.validate().is_ok());
}

#[test]
fn serializes_every_target_with_its_display_name() -> TestResult {
    for target in Target::ALL {
        let json = serde_json::to_string(target)?;
        assert_eq!(json, format!("\"{target}\""));

        let parsed: Target = serde_json::from_str(&json)?;
        assert_eq!(parsed, *target);
    }

    Ok(())
}

#[test]
fn rejects_unsupported_schema_versions() {
    for found in [
        TargetPackVersion::CURRENT.0 - 1,
        TargetPackVersion::CURRENT.0 + 1,
    ] {
        let mut manifest = valid_manifest();
        manifest.schema_version = TargetPackVersion(found);

        assert!(matches!(
            manifest.validate(),
            Err(TargetPackError::UnsupportedSchemaVersion {
                expected,
                found: rejected,
            }) if expected == TargetPackVersion::CURRENT.0 && rejected == found
        ));
    }
}

#[test]
fn rejects_blank_appd_versions() {
    let mut manifest = valid_manifest();
    manifest.appd_version = "  ".to_owned();

    assert!(matches!(
        manifest.validate(),
        Err(TargetPackError::MissingVersion)
    ));
}

#[test]
fn rejects_manifests_without_artifacts() {
    let mut manifest = valid_manifest();
    manifest.artifacts.clear();

    assert!(matches!(
        manifest.validate(),
        Err(TargetPackError::MissingArtifacts)
    ));
}

#[test]
fn rejects_empty_artifact_paths() {
    let mut manifest = valid_manifest();
    manifest.artifacts[0].path.clear();

    assert!(matches!(
        manifest.validate(),
        Err(TargetPackError::EmptyArtifactPath)
    ));
}

#[test]
fn rejects_absolute_and_parent_artifact_paths() -> TestResult {
    let absolute_path = std::env::current_dir()?
        .join("appd-runtime")
        .display()
        .to_string();

    assert_unsafe_artifact_path(&absolute_path);
    assert_unsafe_artifact_path("../appd-runtime");

    Ok(())
}

#[test]
fn rejects_windows_absolute_artifact_paths_on_any_host() {
    for path in [
        r"C:\appd\appd-runtime.exe",
        r"\\server\share\appd-runtime.exe",
    ] {
        assert_unsafe_artifact_path(path);
    }
}

#[test]
fn round_trips_manifest_json_without_losing_contract_fields() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let manifest_path = temp_dir.path().join("target-pack.json");
    let manifest = valid_manifest();

    write_manifest(&manifest_path, &manifest)?;
    let json = fs::read_to_string(&manifest_path)?;
    assert!(json.contains("\"target\": \"ios-arm64\""));
    assert!(!json.contains("sha256"));

    let loaded = load_manifest(&manifest_path)?;
    assert_eq!(loaded, manifest);

    Ok(())
}

#[test]
fn load_manifest_rejects_contract_invalid_json() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let manifest_path = temp_dir.path().join("target-pack.json");
    fs::write(
        &manifest_path,
        r#"{
  "schemaVersion": 12,
  "appdVersion": "0.1.0",
  "target": "ios-arm64",
  "artifacts": [{"kind": "runtimeLibrary", "path": "../appd-runtime"}],
  "requiredTools": []
}"#,
    )?;

    assert!(matches!(
        load_manifest(&manifest_path),
        Err(TargetPackError::UnsafeArtifactPath(path)) if path == "../appd-runtime"
    ));

    Ok(())
}
