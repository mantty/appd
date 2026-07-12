#![deny(missing_docs)]

//! `appd` native runtime host components.

pub mod assets;
pub mod certs;
pub mod host;
#[cfg(feature = "native-shell")]
pub mod platform;
pub mod wrangler_config;

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::certs::{CertificateBundle, CertificatePaths};

/// Runtime result type.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Errors produced by runtime host components.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Operating-system IO failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Certificate generation failed.
    #[error(transparent)]
    CertificateGeneration(#[from] rcgen::Error),
    /// JSON generation failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Bare runtime integration failed.
    #[cfg(feature = "bare-runtime")]
    #[error(transparent)]
    Bare(#[from] appd_bare::Error),
    /// The system clock is earlier than the Unix epoch.
    #[error("system clock is earlier than the Unix epoch")]
    SystemClockBeforeUnixEpoch,
    /// A filesystem path could not be represented in UTF-8.
    #[error("path is not valid UTF-8: {0}")]
    InvalidUtf8Path(PathBuf),
    /// Static asset routing configuration is not valid.
    #[error("invalid asset configuration: {0}")]
    InvalidAssetConfig(String),
    /// No Wrangler configuration file could be found.
    #[error("wrangler config not found starting from {0}")]
    WranglerConfigNotFound(PathBuf),
    /// A Wrangler configuration file uses an unsupported format.
    #[error("unsupported wrangler config format: {0}")]
    UnsupportedWranglerConfigFormat(PathBuf),
    /// A Wrangler configuration file is syntactically invalid.
    #[error("invalid wrangler config {path}: {message}")]
    InvalidWranglerConfig {
        /// Path to the invalid configuration file.
        path: PathBuf,
        /// Parser or validation error details.
        message: String,
    },
    /// appd needs a field that is not present in the Wrangler configuration.
    #[error("wrangler config {path} is missing required field {field}")]
    MissingWranglerConfigField {
        /// Path to the configuration file.
        path: PathBuf,
        /// Name of the missing field.
        field: &'static str,
    },
    /// An executable path did not include a parent directory.
    #[error("path has no parent directory: {0}")]
    MissingParentDirectory(PathBuf),
    /// A packaged runtime did not include its Bare addon frameworks.
    #[error("Bare addon frameworks are missing near: {0}")]
    MissingAddonsDirectory(PathBuf),
}

/// Generate or load cached runtime certificates in a work directory.
///
/// # Errors
///
/// Returns an error when certificate generation, cache loading, or cache writing fails.
pub fn ensure_certificates(work_dir: impl AsRef<Path>) -> RuntimeResult<CertificateBundle> {
    let work_dir = work_dir.as_ref();
    if CertificatePaths::all_exist(work_dir)
        && let Ok(bundle) = CertificateBundle::load_cached(work_dir)
        && bundle.cached_material_is_current(time::OffsetDateTime::now_utc())
    {
        return Ok(bundle);
    }
    generate_and_cache_certificates(work_dir)
}

fn generate_and_cache_certificates(work_dir: &Path) -> RuntimeResult<CertificateBundle> {
    let bundle = CertificateBundle::generate()?;
    bundle.write_all(work_dir)?;
    Ok(bundle)
}

/// Return the URL the native `WebView` should load for a local backend port.
#[must_use]
pub fn frontend_url(port: u16) -> String {
    format!("https://localhost:{port}/")
}
