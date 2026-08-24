use std::path::PathBuf;

use thiserror::Error;

/// Result type for packaged app operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Failures preparing or reading a packaged app.
#[derive(Debug, Error)]
pub enum Error {
    /// Operating-system IO failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON encoding or decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A filesystem path could not be represented in UTF-8.
    #[error("path is not valid UTF-8: {0}")]
    InvalidUtf8Path(PathBuf),
    /// Static asset routing configuration is not valid.
    #[error("invalid asset configuration: {0}")]
    InvalidAssetConfig(String),
    /// A Wrangler module rule is not valid.
    #[error("invalid module rule: {0}")]
    InvalidModuleRule(String),
    /// No Wrangler configuration file could be found.
    #[error("wrangler config not found starting from {0}")]
    ConfigNotFound(PathBuf),
    /// A Wrangler configuration file uses an unsupported format.
    #[error("unsupported wrangler config format: {0}")]
    UnsupportedConfigFormat(PathBuf),
    /// A Wrangler configuration file is syntactically invalid.
    #[error("invalid wrangler config {path}: {message}")]
    InvalidConfig {
        /// Path to the invalid configuration file.
        path: PathBuf,
        /// Parser or validation error details.
        message: String,
    },
    /// appd needs a field that is not present in the Wrangler configuration.
    #[error("wrangler config {path} is missing required field {field}")]
    MissingConfigField {
        /// Path to the configuration file.
        path: PathBuf,
        /// Name of the missing field.
        field: &'static str,
    },
}

/// Return whether a name can be used as an appd app label.
///
/// Names become one DNS label of the app's `appd.local` host.
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

/// Return the `appd.local` host for an app name.
#[must_use]
pub fn app_host(name: &str) -> Option<String> {
    let name = name.to_ascii_lowercase();
    is_valid_app_name(&name).then(|| format!("{name}.appd.local"))
}

#[cfg(test)]
mod tests {
    use super::{app_host, is_valid_app_name};

    #[test]
    fn accepts_one_lower_case_dns_label() {
        assert!(is_valid_app_name("my-app"));
        assert!(!is_valid_app_name(""));
        assert!(!is_valid_app_name("-leading"));
        assert!(!is_valid_app_name("trailing-"));
        assert!(!is_valid_app_name("Upper"));
        assert!(!is_valid_app_name(&"a".repeat(64)));
    }

    #[test]
    fn derives_the_appd_local_host_from_an_app_name() {
        assert_eq!(app_host("my-app").as_deref(), Some("my-app.appd.local"));
        assert_eq!(app_host("Invalid").as_deref(), Some("invalid.appd.local"));
        assert_eq!(app_host("not valid"), None);
    }
}
