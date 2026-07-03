//! Shared runtime startup used by platform shells.

use std::path::{Path, PathBuf};
#[cfg(feature = "workerd-ffi")]
use std::thread;

#[cfg(feature = "workerd-ffi")]
use crate::certs::CertificateBundle;
#[cfg(feature = "workerd-ffi")]
use crate::{LocalBackend, ensure_certificates};
use crate::{RuntimeError, RuntimeResult};

/// Runtime state that must be retained for the app lifetime.
#[cfg(feature = "workerd-ffi")]
#[derive(Debug)]
pub struct StartedRuntime {
    /// Local HTTPS port exposed to the platform `WebView`.
    pub port: u16,
    /// Certificate material used by platform authentication hooks.
    pub certificates: CertificateBundle,
    workerd_thread: thread::JoinHandle<RuntimeResult<()>>,
}

/// Packaged runtime material prepared before `workerd` starts.
#[cfg(feature = "workerd-ffi")]
#[derive(Debug)]
pub struct PreparedRuntime {
    /// Packaged app work directory.
    pub work_dir: PathBuf,
    /// Generated or cached certificate material.
    pub certificates: CertificateBundle,
}

#[cfg(feature = "workerd-ffi")]
impl StartedRuntime {
    /// Consume the runtime and keep its background threads alive forever.
    ///
    /// Platform event loops normally never return. This helper exists for
    /// command-line hosts that need to block explicitly.
    ///
    /// # Errors
    ///
    /// Returns an error if the workerd background thread exits with an error.
    pub fn join(self) -> RuntimeResult<()> {
        join_runtime_thread(self.workerd_thread)
    }
}

/// Return the packaged app work directory next to a desktop executable.
///
/// # Errors
///
/// Returns an error when the executable path does not have a parent directory.
pub fn work_dir_next_to_exe(exe_path: &Path) -> RuntimeResult<PathBuf> {
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| RuntimeError::MissingParentDirectory(exe_path.to_owned()))?;
    Ok(exe_dir.join("app"))
}

/// Return the packaged app work directory inside an Apple bundle resource path.
#[must_use]
pub fn work_dir_in_apple_resources(resource_path: &Path) -> PathBuf {
    resource_path.join("app")
}

pub(crate) fn work_dir_next_to_current_exe_or_default() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| work_dir_next_to_exe(&path).ok())
        .unwrap_or_else(|| PathBuf::from("app"))
}

/// Return the generated `workerd` config path for a work directory.
#[must_use]
pub fn config_path(work_dir: &Path) -> PathBuf {
    work_dir.join("config.capnp")
}

/// Start `workerd` and the loopback socket bridge for a packaged app.
///
/// # Errors
///
/// Returns an error when certificates cannot be generated or loaded, workerd
/// cannot be started, or the loopback listener cannot be bound.
#[cfg(feature = "workerd-ffi")]
pub fn start_workerd_bridge(work_dir: impl AsRef<Path>) -> RuntimeResult<StartedRuntime> {
    let prepared = prepare_workerd_bridge(work_dir)?;
    start_prepared_workerd_bridge(prepared)
}

/// Generate or load certificate material before `workerd` is started.
///
/// # Errors
///
/// Returns an error when certificates cannot be generated, loaded, or cached.
#[cfg(feature = "workerd-ffi")]
pub fn prepare_workerd_bridge(work_dir: impl AsRef<Path>) -> RuntimeResult<PreparedRuntime> {
    let work_dir = work_dir.as_ref();
    let certificates = ensure_certificates(work_dir)?;
    Ok(PreparedRuntime {
        work_dir: work_dir.to_owned(),
        certificates,
    })
}

/// Start `workerd` and the loopback socket bridge from prepared runtime material.
///
/// # Errors
///
/// Returns an error when workerd cannot be started or the loopback listener
/// cannot be bound.
#[cfg(feature = "workerd-ffi")]
pub fn start_prepared_workerd_bridge(prepared: PreparedRuntime) -> RuntimeResult<StartedRuntime> {
    use crate::workerd_ffi::WorkerdFfi;

    let backend = LocalBackend::bind_loopback()?;
    let port = backend.local_port();
    let listener = backend.into_listener();
    let workerd_thread = WorkerdFfi::start(
        config_path(&prepared.work_dir),
        &prepared.work_dir,
        listener,
    )?;

    Ok(StartedRuntime {
        port,
        certificates: prepared.certificates,
        workerd_thread,
    })
}

#[cfg(feature = "workerd-ffi")]
fn join_runtime_thread(thread: thread::JoinHandle<RuntimeResult<()>>) -> RuntimeResult<()> {
    thread
        .join()
        .map_err(|_| RuntimeError::RuntimeThreadPanicked)?
}
