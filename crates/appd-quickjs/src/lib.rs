#![deny(missing_docs)]

//! `QuickJS` runtime integration for appd.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[cfg(feature = "native")]
mod native;
mod worker;

/// Runtime result type.
pub type Result<T> = std::result::Result<T, Error>;

/// `QuickJS` runtime failures.
#[derive(Debug, Error)]
pub enum Error {
    /// Runtime configuration could not be serialized or decoded.
    #[error(transparent)]
    Configuration(#[from] serde_json::Error),
    /// The JavaScript engine rejected an operation.
    #[error("QuickJS operation failed: {0}")]
    Engine(String),
    /// Native IO failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The TLS gateway rejected an operation.
    #[cfg(feature = "native")]
    #[error("TLS operation failed: {0}")]
    Tls(String),
    /// Runtime startup failed.
    #[error("QuickJS startup failed: {0}")]
    Startup(String),
}

/// Certificate paths consumed by the local HTTPS gateway.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Certificates {
    /// Certificate authority PEM file.
    pub ca: PathBuf,
    /// Server certificate PEM file.
    pub certificate: PathBuf,
    /// Server private-key PEM file.
    pub private_key: PathBuf,
}

/// Static asset service paths.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Assets {
    /// Asset routing manifest.
    pub manifest: PathBuf,
    /// Root directory containing static assets.
    pub root: PathBuf,
}

/// Configuration passed to the `QuickJS` runtime.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeConfig {
    /// Optional static asset service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<Assets>,
    /// Directory containing the app-private Worker cache.
    pub cache: PathBuf,
    /// Loopback HTTPS certificates.
    pub certificates: Certificates,
    /// Text and JSON Worker environment bindings.
    pub environment: BTreeMap<String, Value>,
    /// Stable HTTPS hostname used by the `WebView`.
    pub host: String,
    /// Require `WebView` client authentication.
    pub require_client_certificate: bool,
    /// Requested port, or zero for an operating-system assigned port.
    pub port: u16,
}

/// Compile a bundled Worker module to `QuickJS` bytecode.
///
/// The input must be a self-contained ES module without unresolved imports.
///
/// # Errors
///
/// Returns an error when `QuickJS` cannot compile or serialize the module.
pub fn compile_worker(source: &[u8]) -> Result<Vec<u8>> {
    worker::compile(source)
}

/// A running `QuickJS` application.
#[cfg(feature = "native")]
pub struct QuickJsRuntime {
    runtime: native::Runtime,
}

#[cfg(feature = "native")]
impl QuickJsRuntime {
    /// Start an application and wait for its HTTPS listener.
    ///
    /// # Errors
    ///
    /// Returns an error when configuration, certificates, or the gateway is
    /// invalid.
    pub fn start(bundle: &[u8], config: &RuntimeConfig) -> Result<Self> {
        if bundle.is_empty() {
            return Err(Error::Startup("Worker bytecode is empty".to_owned()));
        }
        Ok(Self {
            runtime: native::Runtime::start(bundle, config.clone())?,
        })
    }

    /// Return the loopback HTTPS port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.runtime.port()
    }

    /// Stop new request dispatch and close active gateway connections.
    ///
    /// # Errors
    ///
    /// This operation currently cannot fail after startup.
    pub fn suspend(&self, _linger: i32) -> Result<()> {
        self.runtime.suspend();
        Ok(())
    }

    /// Resume request dispatch.
    ///
    /// # Errors
    ///
    /// This operation currently cannot fail after startup.
    pub fn resume(&self) -> Result<()> {
        self.runtime.resume();
        Ok(())
    }
}

#[cfg(feature = "native")]
impl std::fmt::Debug for QuickJsRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QuickJsRuntime")
            .field("port", &self.port())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::compile_worker;
    use rquickjs::{Context, Module, Runtime};

    #[test]
    fn compiles_and_loads_a_module_bytecode_blob() -> Result<(), Box<dyn std::error::Error>> {
        let bytecode = compile_worker(b"export const value = 42;")?;
        let runtime = Runtime::new()?;
        let context = Context::full(&runtime)?;

        context.with(|ctx| -> Result<(), rquickjs::Error> {
            let module = unsafe { Module::load(ctx.clone(), &bytecode) }?;
            let (module, evaluation) = module.eval()?;
            evaluation.finish::<()>()?;
            let value: i32 = module.get("value")?;
            assert_eq!(value, 42);
            Ok(())
        })?;
        Ok(())
    }
}

#[cfg(all(test, feature = "native"))]
mod native_tests {
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    use super::{Certificates, QuickJsRuntime, RuntimeConfig, compile_worker};

    #[test]
    fn suspends_and_resumes_the_public_runtime() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let bundle =
            compile_worker(br"export default { async fetch() { return new Response('ok'); } };")?;
        let runtime = QuickJsRuntime::start(
            &bundle,
            &RuntimeConfig {
                assets: None,
                cache: directory.path().join("cache"),
                certificates: Certificates {
                    ca: directory.path().join("ca.pem"),
                    certificate: directory.path().join("certificate.pem"),
                    private_key: directory.path().join("private-key.pem"),
                },
                environment: BTreeMap::new(),
                host: "example.test".to_owned(),
                require_client_certificate: false,
                port: 0,
            },
        )?;

        runtime.suspend(-1)?;
        let mut connection = TcpStream::connect(("127.0.0.1", runtime.port()))?;
        connection.set_read_timeout(Some(Duration::from_millis(100)))?;
        connection
            .write_all(b"CONNECT example.test:443 HTTP/1.1\r\nHost: example.test:443\r\n\r\n")?;
        let mut response = [0; 64];
        let Err(error) = connection.read(&mut response) else {
            return Err("suspended runtime responded".into());
        };
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ));

        runtime.resume()?;
        connection.set_read_timeout(Some(Duration::from_secs(2)))?;
        let bytes = connection.read(&mut response)?;
        assert!(std::str::from_utf8(&response[..bytes])?.starts_with("HTTP/1.1 200"));
        Ok(())
    }
}
