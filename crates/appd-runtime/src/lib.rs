#![deny(missing_docs)]

//! `appd` native runtime host components.

pub mod assets;
pub mod capnp_support;
pub mod certs;
pub mod host;
pub mod platform;
pub mod workerd_config;
pub mod wrangler_config;

use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
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
    /// Certificate parsing failed.
    #[error("certificate parsing failed: {0}")]
    CertificateParsing(String),
    /// PKCS#12 identity generation failed.
    #[error("pkcs12 identity generation failed: {0}")]
    Pkcs12Generation(#[from] p12_keystore::error::Error),
    /// JSON generation failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Cap'n Proto message handling failed.
    #[error(transparent)]
    Capnp(#[from] capnp::Error),
    /// An opaque Cap'n Proto payload was too large for the wire type.
    #[error("capnp payload is too large: {0} bytes")]
    CapnpPayloadTooLarge(usize),
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
    /// A path passed to a C API contained an interior NUL byte.
    #[error("path contains an interior nul byte: {0}")]
    InteriorNul(String),
    /// The operating system returned an invalid socket handle.
    #[error("invalid socket handle")]
    InvalidSocket,
    /// An executable path did not include a parent directory.
    #[error("path has no parent directory: {0}")]
    MissingParentDirectory(PathBuf),
    /// A runtime background thread panicked.
    #[error("runtime background thread panicked")]
    RuntimeThreadPanicked,
}

/// Loopback listener used by the WebView-to-workerd bridge.
pub struct LocalBackend {
    listener: TcpListener,
    port: u16,
}

impl LocalBackend {
    /// Bind to `127.0.0.1:0` and let the OS choose an available port.
    ///
    /// # Errors
    ///
    /// Returns an error when the loopback socket cannot be bound or inspected.
    pub fn bind_loopback() -> RuntimeResult<Self> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    /// Return the OS-assigned listener port.
    #[must_use]
    pub fn local_port(&self) -> u16 {
        self.port
    }

    /// Consume the backend and return the listener for workerd ownership.
    #[must_use]
    pub fn into_listener(self) -> TcpListener {
        self.listener
    }
}

/// Generate or load cached runtime certificates in a work directory.
///
/// # Errors
///
/// Returns an error when certificate generation, cache loading, or cache writing
/// fails.
pub fn ensure_certificates(work_dir: impl AsRef<Path>) -> RuntimeResult<CertificateBundle> {
    let work_dir = work_dir.as_ref();
    if CertificatePaths::all_exist(work_dir) {
        let bundle = CertificateBundle::load_cached(work_dir)?;
        if bundle.cached_material_is_current(time::OffsetDateTime::now_utc()) {
            return Ok(bundle);
        }
    }
    generate_and_cache_certificates(work_dir)
}

fn generate_and_cache_certificates(work_dir: &Path) -> RuntimeResult<CertificateBundle> {
    let bundle = CertificateBundle::generate()?;
    bundle.write_all(work_dir)?;
    Ok(bundle)
}

/// Return the URL the native `WebView` should load for a local backend port.
///
/// Windows uses `127.0.0.1` to avoid slow IPv6-first `localhost` resolution in
/// `WebView2`. Other platforms use `localhost`, matching the certificate SAN.
#[must_use]
pub fn frontend_url(port: u16, windows: bool) -> String {
    let host = if windows { "127.0.0.1" } else { "localhost" };
    format!("https://{host}:{port}/")
}

/// Return a platform-specific numeric socket identifier for diagnostics/tests.
///
/// Returns `0` if the handle cannot be represented as a `usize`; callers must
/// not use this helper for ownership transfer or control flow.
#[must_use]
pub fn platform_socket_id(stream: &TcpStream) -> usize {
    socket_platform::socket_id(stream)
}

#[cfg(unix)]
mod socket_platform {
    use std::net::TcpStream;
    use std::os::fd::AsRawFd;

    pub(super) fn socket_id(stream: &TcpStream) -> usize {
        usize::try_from(stream.as_raw_fd()).unwrap_or_default()
    }
}

#[cfg(windows)]
mod socket_platform {
    use std::net::TcpStream;
    use std::os::windows::io::AsRawSocket;

    pub(super) fn socket_id(stream: &TcpStream) -> usize {
        usize::try_from(stream.as_raw_socket()).unwrap_or_default()
    }
}

/// Workerd C-ABI bridge implementation.
#[cfg(feature = "workerd-ffi")]
pub mod workerd_ffi {
    use std::ffi::CString;
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::thread;

    use crate::{RuntimeError, RuntimeResult};

    #[cfg(not(feature = "workerd-test-stubs"))]
    unsafe extern "C" {
        #[link_name = "appd_workerd_serve"]
        fn appd_workerd_serve_raw(
            config_path: *const std::ffi::c_char,
            working_dir: *const std::ffi::c_char,
            listener_fd: usize,
        ) -> std::ffi::c_int;
        #[link_name = "appd_workerd_wait_ready"]
        fn appd_workerd_wait_ready_raw() -> std::ffi::c_int;
    }

    #[cfg(feature = "workerd-test-stubs")]
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(feature = "workerd-test-stubs")]
    static LAST_LISTENER_SOCKET: AtomicUsize = AtomicUsize::new(0);

    #[cfg(feature = "workerd-test-stubs")]
    unsafe fn appd_workerd_serve_raw(
        _: *const std::ffi::c_char,
        _: *const std::ffi::c_char,
        listener_fd: usize,
    ) -> std::ffi::c_int {
        LAST_LISTENER_SOCKET.store(listener_fd, Ordering::SeqCst);
        -1
    }

    #[cfg(feature = "workerd-test-stubs")]
    unsafe fn appd_workerd_wait_ready_raw() -> std::ffi::c_int {
        while LAST_LISTENER_SOCKET.load(Ordering::SeqCst) == 0 {
            thread::yield_now();
        }
        0
    }

    /// Clear the listener socket captured by the workerd test stub.
    #[cfg(feature = "workerd-test-stubs")]
    pub fn reset_stub_listener_socket() {
        LAST_LISTENER_SOCKET.store(0, Ordering::SeqCst);
    }

    /// Return the last listener socket transferred to the workerd test stub.
    #[cfg(feature = "workerd-test-stubs")]
    #[must_use]
    pub fn stub_listener_socket() -> usize {
        LAST_LISTENER_SOCKET.load(Ordering::SeqCst)
    }

    /// C-ABI workerd launcher.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct WorkerdFfi;

    impl WorkerdFfi {
        /// Start workerd in-process on a background thread and wait until its
        /// inherited loopback listener is ready.
        ///
        /// # Errors
        ///
        /// Returns an error when paths cannot be represented for the C ABI, the
        /// listener handle cannot be transferred, or workerd reports startup
        /// failure.
        pub fn start(
            config_path: impl AsRef<Path>,
            working_dir: impl AsRef<Path>,
            listener: TcpListener,
        ) -> RuntimeResult<thread::JoinHandle<RuntimeResult<()>>> {
            let config_path = path_to_cstring(config_path.as_ref())?;
            let working_dir = path_to_cstring(working_dir.as_ref())?;
            let listener_fd = take_listener_socket(listener)?;

            let handle = thread::spawn(move || {
                let rc = unsafe {
                    appd_workerd_serve_raw(config_path.as_ptr(), working_dir.as_ptr(), listener_fd)
                };
                if rc == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::other(format!("workerd exited with status {rc}")).into())
                }
            });

            // SAFETY: appd_workerd_wait_ready only observes workerd global
            // readiness after appd_workerd_serve has been started above.
            let ready_status = unsafe { appd_workerd_wait_ready_raw() };
            if ready_status != 0 {
                let _ = handle.join();
                return Err(std::io::Error::other(format!(
                    "workerd startup failed with status {ready_status}"
                ))
                .into());
            }

            Ok(handle)
        }
    }

    fn path_to_cstring(path: &Path) -> RuntimeResult<CString> {
        let text = path
            .to_str()
            .ok_or_else(|| RuntimeError::InvalidUtf8Path(PathBuf::from(path)))?;
        CString::new(text).map_err(|error| RuntimeError::InteriorNul(error.to_string()))
    }

    #[cfg(unix)]
    fn take_listener_socket(listener: TcpListener) -> RuntimeResult<usize> {
        use std::os::fd::{AsRawFd, IntoRawFd};

        let fd = usize::try_from(listener.as_raw_fd())
            .map_err(|_| crate::RuntimeError::InvalidSocket)?;
        let raw = listener.into_raw_fd();
        debug_assert_eq!(usize::try_from(raw).ok(), Some(fd));
        Ok(fd)
    }

    #[cfg(windows)]
    fn take_listener_socket(listener: TcpListener) -> RuntimeResult<usize> {
        use std::os::windows::io::IntoRawSocket;

        let raw = listener.into_raw_socket();
        usize::try_from(raw).map_err(|_| crate::RuntimeError::InvalidSocket)
    }
}
