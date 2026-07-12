#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Target-pack metadata and validation for `appd` runtime artifacts.

use std::fmt;
use std::fs;
use std::path::{Component, Path};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// The current target-pack manifest schema version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TargetPackVersion(pub u32);

impl TargetPackVersion {
    /// Current manifest schema version.
    pub const CURRENT: Self = Self(3);
}

/// Supported `appd` runtime target triples.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Target {
    /// Physical iOS devices.
    IosArm64,
    /// Apple Silicon macOS.
    MacosArm64,
}

impl Target {
    /// All supported targets in stable display order.
    pub const ALL: &'static [Self] = &[Self::IosArm64, Self::MacosArm64];

    fn manifest_name(self) -> &'static str {
        match self {
            Self::IosArm64 => "ios-arm64",
            Self::MacosArm64 => "macos-arm64",
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.manifest_name())
    }
}

impl Serialize for Target {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(serde::de::Error::custom)
    }
}

impl FromStr for Target {
    type Err = TargetPackError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|target| target.manifest_name() == value)
            .ok_or_else(|| TargetPackError::UnknownTarget(value.to_owned()))
    }
}

/// Artifact categories that a target pack can expose to the CLI.
///
/// Unknown artifact kinds intentionally fail deserialization. Additive artifact
/// kinds must bump [`TargetPackVersion`] so older CLIs reject packs they cannot
/// fully understand.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactKind {
    /// Executable runtime binary.
    RuntimeExecutable,
    /// Compiled appd JavaScript runtime modules used by the host packer.
    RuntimeJavaScriptDirectory,
    /// Standalone host-side Bare bundle packer.
    BarePackExecutable,
    /// Host-side JavaScript compiler used to produce `CommonJS` worklets.
    EsbuildExecutable,
}

/// A single file or directory provided by a target pack.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    /// The artifact role.
    pub kind: ArtifactKind,
    /// Path relative to the target-pack root.
    pub path: String,
    /// Optional SHA-256 hex digest for file artifacts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// Versioned manifest included in each `appd` target pack.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetPackManifest {
    /// Schema version for this manifest.
    pub schema_version: TargetPackVersion,
    /// `appd` release version that produced the target pack.
    pub appd_version: String,
    /// Native platform target.
    pub target: Target,
    /// Runtime artifacts available in this pack.
    pub artifacts: Vec<Artifact>,
    /// Host tools required to consume this pack locally.
    pub required_tools: Vec<String>,
}

impl TargetPackManifest {
    /// Validate the manifest contract before a CLI consumes the target pack.
    ///
    /// # Errors
    ///
    /// Returns an error when required fields are missing, the schema version is
    /// unsupported, an artifact path escapes the pack root, or a digest is not a
    /// lowercase SHA-256 hex value.
    pub fn validate(&self) -> Result<(), TargetPackError> {
        if self.schema_version != TargetPackVersion::CURRENT {
            return Err(TargetPackError::UnsupportedSchemaVersion {
                expected: TargetPackVersion::CURRENT.0,
                found: self.schema_version.0,
            });
        }

        if self.appd_version.trim().is_empty() {
            return Err(TargetPackError::MissingVersion);
        }

        if self.artifacts.is_empty() {
            return Err(TargetPackError::MissingArtifacts);
        }

        for artifact in &self.artifacts {
            validate_relative_path(&artifact.path)?;
            if let Some(sha256) = &artifact.sha256 {
                validate_sha256(sha256)?;
            }
        }

        Ok(())
    }
}

/// Load a target-pack manifest from JSON.
///
/// # Errors
///
/// Returns an error when the file cannot be read, the JSON cannot be parsed,
/// or the decoded manifest violates the target-pack contract.
pub fn load_manifest(path: impl AsRef<Path>) -> Result<TargetPackManifest, TargetPackError> {
    let content = fs::read_to_string(path.as_ref())?;
    let manifest = serde_json::from_str(&content)?;
    TargetPackManifest::validate(&manifest)?;
    Ok(manifest)
}

/// Write a target-pack manifest as pretty JSON.
///
/// # Errors
///
/// Returns an error when the manifest is invalid, the JSON cannot be
/// serialized, or the destination file cannot be written.
pub fn write_manifest(
    path: impl AsRef<Path>,
    manifest: &TargetPackManifest,
) -> Result<(), TargetPackError> {
    manifest.validate()?;
    let content = serde_json::to_string_pretty(manifest)?;
    fs::write(path.as_ref(), content)?;
    Ok(())
}

/// Target-pack parsing and validation failures.
#[derive(Debug, Error)]
pub enum TargetPackError {
    /// Manifest uses an unsupported schema version.
    #[error("unsupported target-pack schema version {found}; expected {expected}")]
    UnsupportedSchemaVersion {
        /// Expected schema version.
        expected: u32,
        /// Found schema version.
        found: u32,
    },
    /// Unknown target name.
    #[error("unknown target '{0}'")]
    UnknownTarget(String),
    /// `appd` version was empty.
    #[error("appdVersion must not be empty")]
    MissingVersion,
    /// Manifest had no artifacts.
    #[error("target pack must contain at least one artifact")]
    MissingArtifacts,
    /// Artifact path was empty.
    #[error("artifact path must not be empty")]
    EmptyArtifactPath,
    /// Artifact path was absolute or escaped the pack root.
    #[error("artifact path must stay inside the target pack: {0}")]
    UnsafeArtifactPath(String),
    /// Artifact SHA-256 digest was not lowercase hex.
    #[error("sha256 must be 64 lowercase hex characters: {0}")]
    InvalidSha256(String),
    /// File IO failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON parsing or serialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn validate_relative_path(path: &str) -> Result<(), TargetPackError> {
    if path.is_empty() {
        return Err(TargetPackError::EmptyArtifactPath);
    }

    if path.contains('\\') || has_windows_drive_prefix(path) {
        return Err(TargetPackError::UnsafeArtifactPath(path.to_owned()));
    }

    for component in Path::new(path).components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(TargetPackError::UnsafeArtifactPath(path.to_owned()));
            }
        }
    }

    Ok(())
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn validate_sha256(value: &str) -> Result<(), TargetPackError> {
    let is_valid = value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));

    if is_valid {
        Ok(())
    } else {
        Err(TargetPackError::InvalidSha256(value.to_owned()))
    }
}
