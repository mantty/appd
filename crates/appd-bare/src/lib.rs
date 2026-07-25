#![deny(missing_docs)]

//! Safe lifecycle wrapper for the appd Bare worklet.

use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

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
    handle: std::ptr::NonNull<std::ffi::c_void>,
    port: u16,
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
        native::start(bundle, &config)
    }

    /// Return the loopback HTTPS port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Suspend JavaScript execution, allowing `linger` milliseconds to settle work.
    ///
    /// # Errors
    ///
    /// Returns an error when Bare rejects the lifecycle transition.
    pub fn suspend(&self, linger: i32) -> Result<()> {
        native::status(unsafe { native::appd_bare_runtime_suspend(self.handle.as_ptr(), linger) })
    }

    /// Resume JavaScript execution.
    ///
    /// # Errors
    ///
    /// Returns an error when Bare rejects the lifecycle transition.
    pub fn resume(&self) -> Result<()> {
        native::status(unsafe { native::appd_bare_runtime_resume(self.handle.as_ptr()) })
    }
}

#[cfg(feature = "native")]
impl Drop for BareRuntime {
    fn drop(&mut self) {
        unsafe { native::appd_bare_runtime_terminate(self.handle.as_ptr()) };
    }
}

#[cfg(feature = "native")]
unsafe impl Send for BareRuntime {}

#[cfg(feature = "native")]
mod native {
    use std::ffi::{c_char, c_int, c_void};
    use std::ptr::NonNull;

    use super::{BareRuntime, Error, Result};

    const ERROR_CAPACITY: usize = 512;

    #[cfg(not(feature = "test-stubs"))]
    unsafe extern "C" {
        pub(super) fn appd_bare_runtime_start(
            bundle: *const u8,
            bundle_len: usize,
            config: *const u8,
            config_len: usize,
            runtime: *mut *mut c_void,
            port: *mut u16,
            error: *mut c_char,
            error_len: usize,
        ) -> c_int;
        pub(super) fn appd_bare_runtime_suspend(runtime: *mut c_void, linger: c_int) -> c_int;
        pub(super) fn appd_bare_runtime_resume(runtime: *mut c_void) -> c_int;
        pub(super) fn appd_bare_runtime_terminate(runtime: *mut c_void);
    }

    pub(super) fn start(bundle: &[u8], config: &[u8]) -> Result<BareRuntime> {
        let mut handle = std::ptr::null_mut();
        let mut port = 0;
        let mut error = [0 as c_char; ERROR_CAPACITY];
        let status = unsafe {
            appd_bare_runtime_start(
                bundle.as_ptr(),
                bundle.len(),
                config.as_ptr(),
                config.len(),
                &raw mut handle,
                &raw mut port,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if status != 0 {
            return Err(native_error(status, &error));
        }
        let handle = NonNull::new(handle).ok_or_else(|| native_error(-1, &error))?;
        Ok(BareRuntime { handle, port })
    }

    pub(super) fn status(status: i32) -> Result<()> {
        if status == 0 {
            Ok(())
        } else {
            Err(Error::Native {
                status,
                message: "lifecycle transition failed".to_owned(),
            })
        }
    }

    fn native_error(status: i32, error: &[c_char]) -> Error {
        let bytes: Vec<u8> = error
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| byte.to_ne_bytes()[0])
            .collect();
        Error::Native {
            status,
            message: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    #[cfg(feature = "test-stubs")]
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn appd_bare_runtime_start(
        _: *const u8,
        _: usize,
        _: *const u8,
        _: usize,
        runtime: *mut *mut c_void,
        port: *mut u16,
        _: *mut c_char,
        _: usize,
    ) -> c_int {
        unsafe {
            runtime.write(NonNull::<u8>::dangling().as_ptr().cast());
            port.write(8443);
        }
        0
    }

    #[cfg(feature = "test-stubs")]
    pub(super) unsafe fn appd_bare_runtime_suspend(_: *mut c_void, _: c_int) -> c_int {
        0
    }

    #[cfg(feature = "test-stubs")]
    pub(super) unsafe fn appd_bare_runtime_resume(_: *mut c_void) -> c_int {
        0
    }

    #[cfg(feature = "test-stubs")]
    pub(super) unsafe fn appd_bare_runtime_terminate(_: *mut c_void) {}
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Assets, Certificates, RuntimeConfig};

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
