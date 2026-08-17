#![deny(missing_docs)]

//! The appd runtime.
//!
//! A per-platform shell owns the application and drives this library: it
//! starts a [`Runtime`] for a packaged app, answers platform TLS challenges
//! with [`Certificates`], and reports operating-system lifecycle changes.

mod certificates;
mod events;
mod material;
#[cfg(feature = "native")]
mod runtime;

use thiserror::Error as Fail;

pub use certificates::{Certificates, Challenge, Decision};
pub use events::Event;
#[cfg(feature = "native")]
pub use runtime::{Config, Runtime};

/// Runtime result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Runtime failures.
#[derive(Debug, Fail)]
pub enum Error {
    /// Operating-system IO failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Certificate generation failed.
    #[error(transparent)]
    Certificate(#[from] rcgen::Error),
    /// The JavaScript runtime failed to start or change state.
    #[cfg(feature = "native")]
    #[error(transparent)]
    QuickJs(#[from] appd_quickjs::Error),
    /// The packaged app contents are invalid.
    #[cfg(feature = "native")]
    #[error(transparent)]
    Bundle(#[from] appd_bundle::Error),
    /// Certificate state is held by a thread that stopped unexpectedly.
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
