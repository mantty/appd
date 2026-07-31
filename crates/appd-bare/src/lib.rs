#![deny(missing_docs)]

//! Safe lifecycle wrapper for the appd Bare worklet.

use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

#[cfg(all(feature = "native", not(feature = "test-stubs")))]
mod native;

#[cfg(all(feature = "native", feature = "test-stubs"))]
#[allow(clippy::unnecessary_wraps, clippy::unused_self)]
mod native {
    use super::Result;

    pub(super) struct Runtime {
        port: u16,
    }

    impl Runtime {
        pub(super) fn start(_: &[u8], _: &[u8]) -> Result<Self> {
            Ok(Self { port: 8443 })
        }

        pub(super) const fn port(&self) -> u16 {
            self.port
        }

        pub(super) const fn suspend(&self, _: i32) -> Result<()> {
            Ok(())
        }

        pub(super) const fn resume(&self) -> Result<()> {
            Ok(())
        }
    }
}

/// Result returned by the Bare integration.
pub type Result<T> = std::result::Result<T, Error>;

/// Bare runtime startup and lifecycle failures.
#[derive(Debug, Error)]
pub enum Error {
    /// Runtime configuration could not be serialized.
    #[error(transparent)]
    Configuration(#[from] serde_json::Error),
    /// The native Bare integration rejected an operation.
    #[error("Bare operation failed with status {status}: {message}")]
    Native {
        /// Native status code.
        status: i32,
        /// Native error message.
        message: String,
    },
    /// The JavaScript runtime could not complete its startup protocol.
    #[error("Bare startup failed: {0}")]
    Startup(String),
}

/// Certificate paths consumed by the Bare HTTPS server.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Certificates {
    /// Certificate authority PEM file.
    pub ca: PathBuf,
    /// Server certificate and private-key PEM file.
    pub identity: PathBuf,
}

/// Static asset service paths.
#[derive(Debug, Serialize)]
pub struct Assets {
    /// Asset routing manifest.
    pub manifest: PathBuf,
    /// Root directory containing static assets.
    pub root: PathBuf,
}

/// Configuration sent to the JavaScript runtime during startup.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeConfig {
    /// Optional static asset service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<Assets>,
    /// Loopback HTTPS certificates.
    pub certificates: Certificates,
    /// Stable HTTPS hostname used by the `WebView`.
    pub host: String,
    /// Require the `WebView` to authenticate to the local gateway when it is network-facing.
    pub require_client_certificate: bool,
    /// Requested port, or zero for an operating-system assigned port.
    pub port: u16,
}

/// A running Bare worklet.
#[cfg(feature = "native")]
pub struct BareRuntime {
    runtime: native::Runtime,
}

#[cfg(feature = "native")]
impl BareRuntime {
    /// Start an app bundle and wait for its HTTPS listener.
    ///
    /// # Errors
    ///
    /// Returns an error when configuration serialization or native startup fails.
    pub fn start(bundle: &[u8], config: &RuntimeConfig) -> Result<Self> {
        let config = serde_json::to_vec(config)?;
        Ok(Self {
            runtime: native::Runtime::start(bundle, &config)?,
        })
    }

    /// Return the loopback HTTPS port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.runtime.port()
    }

    /// Suspend JavaScript execution, allowing `linger` milliseconds to settle work.
    ///
    /// # Errors
    ///
    /// Returns an error when Bare rejects the lifecycle transition.
    pub fn suspend(&self, linger: i32) -> Result<()> {
        self.runtime.suspend(linger)
    }

    /// Resume JavaScript execution.
    ///
    /// # Errors
    ///
    /// Returns an error when Bare rejects the lifecycle transition.
    pub fn resume(&self) -> Result<()> {
        self.runtime.resume()
    }
}

#[cfg(feature = "native")]
impl std::fmt::Debug for BareRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BareRuntime")
            .field("port", &self.port())
            .finish_non_exhaustive()
    }
}

#[cfg(any(all(feature = "native", not(feature = "test-stubs")), test))]
fn parse_startup_reply(reply: &str) -> Result<u16> {
    if let Some(message) = reply.strip_prefix("error ") {
        return Err(Error::Startup(message.to_owned()));
    }
    let Some(value) = reply.strip_prefix("listening ") else {
        return Err(Error::Startup(format!("unexpected reply: {reply}")));
    };
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| Error::Startup("invalid listening port".to_owned()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Assets, Certificates, Error, RuntimeConfig, parse_startup_reply};

    #[test]
    fn parses_listening_port() {
        assert_eq!(
            parse_startup_reply("listening 8443").unwrap_or_default(),
            8443
        );
    }

    #[test]
    fn rejects_invalid_startup_replies() {
        for reply in [
            "listening 0",
            "listening 65536",
            "listening nope",
            "unexpected 8443",
        ] {
            assert!(parse_startup_reply(reply).is_err(), "accepted {reply}");
        }
    }

    #[test]
    fn preserves_reported_startup_error() {
        assert!(matches!(
            parse_startup_reply("error certificate missing"),
            Err(Error::Startup(message)) if message == "certificate missing"
        ));
    }

    #[test]
    fn configuration_matches_javascript_contract() {
        let config = RuntimeConfig {
            assets: Some(Assets {
                manifest: Path::new("assets.json").to_path_buf(),
                root: Path::new("assets").to_path_buf(),
            }),
            certificates: Certificates {
                ca: Path::new("ca.pem").to_path_buf(),
                identity: Path::new("server.identity.pem").to_path_buf(),
            },
            host: "app.appd.local".to_owned(),
            port: 0,
            require_client_certificate: true,
        };
        let json = serde_json::to_value(config).unwrap_or_default();
        assert_eq!(json["certificates"]["identity"], "server.identity.pem");
        assert_eq!(json["requireClientCertificate"], true);
        assert_eq!(json["assets"]["manifest"], "assets.json");
        assert_eq!(json["port"], 0);
    }
}
