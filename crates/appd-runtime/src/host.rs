//! Shared runtime startup used by platform shells.

use std::path::{Path, PathBuf};

#[cfg(feature = "bare-runtime")]
use appd_bare::{Assets, BareRuntime, Certificates, RuntimeConfig};

#[cfg(feature = "bare-runtime")]
use crate::certs::CertificateBundle;
#[cfg(feature = "bare-runtime")]
use crate::certs::CertificatePaths;
#[cfg(feature = "bare-runtime")]
use crate::ensure_certificates;
use crate::{RuntimeError, RuntimeResult};

/// Name of the packaged Bare application bundle.
pub const WORKER_BUNDLE_FILE: &str = "worker.bundle";
/// Name of the static asset routing manifest.
pub const ASSET_MANIFEST_FILE: &str = "asset-manifest.json";
/// Name of the packaged static asset directory.
pub const ASSET_DIRECTORY: &str = "assets";

/// Runtime state retained for the app lifetime.
#[cfg(feature = "bare-runtime")]
pub struct StartedRuntime {
    /// Local HTTPS port exposed to the platform `WebView`.
    pub port: u16,
    /// Certificate material used by platform authentication hooks.
    pub certificates: CertificateBundle,
    runtime: BareRuntime,
}

#[cfg(feature = "bare-runtime")]
impl StartedRuntime {
    /// Suspend JavaScript execution.
    ///
    /// # Errors
    ///
    /// Returns an error when Bare rejects the transition.
    pub fn suspend(&self) -> RuntimeResult<()> {
        self.runtime.suspend(-1)?;
        eprintln!("appd Bare runtime suspended");
        Ok(())
    }

    /// Resume JavaScript execution.
    ///
    /// # Errors
    ///
    /// Returns an error when Bare rejects the transition.
    pub fn resume(&self) -> RuntimeResult<()> {
        self.runtime.resume()?;
        eprintln!("appd Bare runtime resumed");
        Ok(())
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

#[cfg(feature = "native-shell")]
pub(crate) fn work_dir_next_to_current_exe_or_default() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| work_dir_next_to_exe(&path).ok())
        .unwrap_or_else(|| PathBuf::from("app"))
}

/// Return the packaged Bare worker bundle path.
#[must_use]
pub fn bundle_path(work_dir: &Path) -> PathBuf {
    work_dir.join(WORKER_BUNDLE_FILE)
}

/// Start Bare and its loopback mTLS server for a packaged app.
///
/// `packaged_dir` contains immutable bundled code and assets. `state_dir` is
/// writable per-app storage for generated certificate material.
///
/// # Errors
///
/// Returns an error when certificates, bundle loading, or Bare startup fails.
#[cfg(feature = "bare-runtime")]
pub fn start_bare_runtime(
    packaged_dir: impl AsRef<Path>,
    state_dir: impl AsRef<Path>,
) -> RuntimeResult<StartedRuntime> {
    let packaged_dir = packaged_dir.as_ref();
    let state_dir = state_dir.as_ref();
    let certificates = ensure_certificates(state_dir)?;
    let bundle = std::fs::read(bundle_path(packaged_dir))?;
    let config = runtime_config(packaged_dir, state_dir);
    std::env::set_current_dir(addon_directory(packaged_dir)?)?;
    let runtime = BareRuntime::start(&bundle, &config)?;
    eprintln!(
        "appd Bare runtime listening on https://localhost:{}",
        runtime.port()
    );
    Ok(StartedRuntime {
        port: runtime.port(),
        certificates,
        runtime,
    })
}

#[cfg(feature = "bare-runtime")]
fn addon_directory(packaged_dir: &Path) -> RuntimeResult<PathBuf> {
    packaged_dir
        .ancestors()
        .map(|directory| directory.join("Frameworks"))
        .find(|directory| directory.is_dir())
        .ok_or_else(|| RuntimeError::MissingAddonsDirectory(packaged_dir.to_owned()))
}

#[cfg(feature = "bare-runtime")]
fn runtime_config(packaged_dir: &Path, state_dir: &Path) -> RuntimeConfig {
    let manifest = packaged_dir.join(ASSET_MANIFEST_FILE);
    let assets = manifest.is_file().then(|| Assets {
        manifest,
        root: packaged_dir.join(ASSET_DIRECTORY),
    });
    RuntimeConfig {
        assets,
        certificates: Certificates {
            ca: state_dir.join(CertificatePaths::CA_CERT_PEM),
            certificate: state_dir.join(CertificatePaths::SERVER_CERT_PEM),
            private_key: state_dir.join(CertificatePaths::SERVER_KEY_PEM),
        },
        port: 0,
    }
}
