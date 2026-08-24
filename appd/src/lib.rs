#![deny(missing_docs)]

//! The appd runtime.
//!
//! A per-platform shell owns the application and drives this library: it
//! starts a [`Runtime`] for a packaged app, answers platform TLS challenges
//! with [`Certificates`], and reports operating-system lifecycle changes.

#[cfg(feature = "native")]
mod node_fs;
#[cfg(feature = "native")]
mod platform;
mod quickjs;
#[cfg(feature = "native")]
mod runtime;
#[cfg(feature = "native")]
mod vfs;
pub mod worker_package;

use thiserror::Error as Fail;

pub use quickjs::Error as QuickJsError;
pub use quickjs::{compile_module, compile_worker};
#[cfg(feature = "native")]
pub use runtime::{Certificates, Challenge, Config, Decision, Event, Runtime};

/// Runtime result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Runtime failures.
#[derive(Debug, Fail)]
pub enum Error {
    /// Operating-system IO failed.
    #[cfg(feature = "native")]
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Certificate generation failed.
    #[cfg(feature = "native")]
    #[error(transparent)]
    Certificate(#[from] rcgen::Error),
    /// The JavaScript runtime failed to start or change state.
    #[cfg(feature = "native")]
    #[error(transparent)]
    QuickJs(#[from] QuickJsError),
    /// The packaged app contents are invalid.
    #[cfg(feature = "native")]
    #[error(transparent)]
    Bundle(#[from] worker_package::Error),
    /// Certificate state is held by a thread that stopped unexpectedly.
    #[cfg(feature = "native")]
    #[error("certificate state is unavailable")]
    CertificatesUnavailable,
}

/// Return the URL a shell's `WebView` loads.
#[must_use]
pub fn frontend_url(host: &str) -> String {
    format!("https://{host}/")
}

#[cfg(test)]
mod tests {
    #[test]
    fn frontend_url_uses_the_stable_host() {
        assert_eq!(
            super::frontend_url("app.appd.local"),
            "https://app.appd.local/"
        );
    }
}
