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

use crate::certs::CertificateBundle;

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
    /// An Apple app bundle identifier cannot provide a valid local hostname.
    #[error("app bundle identifier has an invalid app name: {0}")]
    InvalidAppName(String),
}

/// Generate or load cached runtime certificates in a work directory.
///
/// # Errors
///
/// Returns an error when certificate generation, cache loading, or cache writing fails.
pub fn ensure_certificates(
    work_dir: impl AsRef<Path>,
    host: &str,
) -> RuntimeResult<CertificateBundle> {
    let work_dir = work_dir.as_ref();
    refresh_certificates(work_dir, host, time::OffsetDateTime::now_utc())
}

/// Return a valid certificate bundle, renewing leaf certificates when due.
///
/// # Errors
///
/// Returns an error when certificate generation, cache loading, or cache writing fails.
pub fn refresh_certificates(
    work_dir: impl AsRef<Path>,
    host: &str,
    now: time::OffsetDateTime,
) -> RuntimeResult<CertificateBundle> {
    let work_dir = work_dir.as_ref();
    let Ok(bundle) = CertificateBundle::load_cached(work_dir) else {
        return renew_or_generate_certificates(work_dir, host, now);
    };
    if !bundle.issuer_is_current(now) {
        return generate_and_cache_certificates(work_dir, host, now);
    }
    if !bundle.cached_material_is_current(now)
        || !bundle.server_certificate_matches_host(host)
        || bundle.leaf_renewal_is_due(now)
    {
        let replacement = bundle.renew_leaves(host, now)?;
        replacement.write_leaves(work_dir)?;
        return Ok(replacement);
    }
    Ok(bundle)
}

fn renew_or_generate_certificates(
    work_dir: &Path,
    host: &str,
    now: time::OffsetDateTime,
) -> RuntimeResult<CertificateBundle> {
    let Ok(issuer) = CertificateBundle::load_issuer(work_dir) else {
        return generate_and_cache_certificates(work_dir, host, now);
    };
    if !issuer.issuer_is_current(now) {
        return generate_and_cache_certificates(work_dir, host, now);
    }
    let replacement = issuer.renew_leaves(host, now)?;
    replacement.write_all(work_dir)?;
    Ok(replacement)
}

fn generate_and_cache_certificates(
    work_dir: &Path,
    host: &str,
    now: time::OffsetDateTime,
) -> RuntimeResult<CertificateBundle> {
    let bundle = CertificateBundle::generate_at(host, now)?;
    bundle.write_all(work_dir)?;
    Ok(bundle)
}

/// Return the stable URL the native `WebView` should load.
#[must_use]
pub fn frontend_url(host: &str) -> String {
    format!("https://{host}/")
}

/// Return whether a name can be used as an appd app label.
#[must_use]
pub fn is_valid_app_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}
