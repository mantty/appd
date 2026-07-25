//! Shared runtime startup used by platform shells.

use std::path::{Path, PathBuf};
#[cfg(feature = "bare-runtime")]
use std::sync::mpsc::{self, Sender};
#[cfg(feature = "bare-runtime")]
use std::thread::{self, JoinHandle};

#[cfg(feature = "bare-runtime")]
use appd_bare::{Assets, BareRuntime, Certificates, RuntimeConfig};

#[cfg(feature = "bare-runtime")]
use crate::certs::CertificatePaths;
#[cfg(feature = "bare-runtime")]
use crate::certs::{CertificateBundle, SharedCertificateBundle};
use crate::{RuntimeError, RuntimeResult};
#[cfg(feature = "bare-runtime")]
use crate::{ensure_certificates, refresh_certificates};

/// Name of the packaged Bare application bundle.
pub const WORKER_BUNDLE_FILE: &str = "worker.bundle";
/// Name of the static asset routing manifest.
pub const ASSET_MANIFEST_FILE: &str = "asset-manifest.json";
/// Name of the packaged static asset directory.
pub const ASSET_DIRECTORY: &str = "assets";

/// Runtime state retained for the app lifetime.
#[cfg(feature = "bare-runtime")]
pub struct StartedRuntime {
    /// Local proxy port exposed to the platform `WebView`.
    pub port: u16,
    /// Stable HTTPS hostname exposed to the platform `WebView`.
    pub host: String,
    state_dir: PathBuf,
    certificates: SharedCertificateBundle,
    _refresher: CertificateRefresher,
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
        self.refresh_certificates()?;
        self.runtime.resume()?;
        eprintln!("appd Bare runtime resumed");
        Ok(())
    }

    fn refresh_certificates(&self) -> RuntimeResult<()> {
        let certificates =
            refresh_certificates(&self.state_dir, &self.host, time::OffsetDateTime::now_utc())?;
        if let Ok(mut current) = self.certificates.write() {
            *current = Some(certificates);
        }
        Ok(())
    }
}

#[cfg(feature = "bare-runtime")]
struct CertificateRefresher {
    stop: Sender<()>,
    task: Option<JoinHandle<()>>,
}

#[cfg(feature = "bare-runtime")]
impl CertificateRefresher {
    fn start(state_dir: PathBuf, host: String, certificates: SharedCertificateBundle) -> Self {
        let (stop, receiver) = mpsc::channel();
        let task = thread::spawn(move || {
            let mut retrying = false;
            loop {
                let delay = if retrying {
                    std::time::Duration::from_hours(1)
                } else {
                    certificates
                        .read()
                        .ok()
                        .and_then(|current| current.as_ref().map(next_renewal_delay))
                        .unwrap_or_else(|| std::time::Duration::from_hours(1))
                };
                if receiver.recv_timeout(delay).is_ok() {
                    break;
                }
                match refresh_certificates(&state_dir, &host, time::OffsetDateTime::now_utc()) {
                    Ok(bundle) => {
                        if let Ok(mut current) = certificates.write() {
                            *current = Some(bundle);
                        }
                        retrying = false;
                    }
                    Err(error) => {
                        retrying = true;
                        eprintln!("appd certificate renewal failed: {error:#}");
                    }
                }
            }
        });
        Self {
            stop,
            task: Some(task),
        }
    }
}

#[cfg(feature = "bare-runtime")]
impl Drop for CertificateRefresher {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}

#[cfg(feature = "bare-runtime")]
fn next_renewal_delay(bundle: &CertificateBundle) -> std::time::Duration {
    bundle.renewal_delay(time::OffsetDateTime::now_utc())
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

#[cfg(feature = "bare-runtime")]
#[cfg(any(target_os = "ios", target_os = "macos"))]
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

/// Start Bare and its loopback HTTPS gateway for a packaged app.
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
    host: &str,
    certificates: SharedCertificateBundle,
) -> RuntimeResult<StartedRuntime> {
    let packaged_dir = packaged_dir.as_ref();
    let state_dir = state_dir.as_ref();
    let certificate_bundle = ensure_certificates(state_dir, host)?;
    if let Ok(mut current) = certificates.write() {
        *current = Some(certificate_bundle);
    }
    let bundle = std::fs::read(bundle_path(packaged_dir))?;
    let config = runtime_config(packaged_dir, state_dir, host);
    let runtime = BareRuntime::start(&bundle, &config)?;
    eprintln!(
        "appd Bare proxy listening on 127.0.0.1:{} for https://{}/",
        runtime.port(),
        host,
    );
    let refresher =
        CertificateRefresher::start(state_dir.to_owned(), host.to_owned(), certificates.clone());
    Ok(StartedRuntime {
        port: runtime.port(),
        host: host.to_owned(),
        state_dir: state_dir.to_owned(),
        certificates,
        _refresher: refresher,
        runtime,
    })
}

#[cfg(feature = "bare-runtime")]
fn runtime_config(packaged_dir: &Path, state_dir: &Path, host: &str) -> RuntimeConfig {
    let manifest = packaged_dir.join(ASSET_MANIFEST_FILE);
    let assets = manifest.is_file().then(|| Assets {
        manifest,
        root: packaged_dir.join(ASSET_DIRECTORY),
    });
    RuntimeConfig {
        assets,
        certificates: Certificates {
            ca: state_dir.join(CertificatePaths::CA_CERT_PEM),
            identity: state_dir.join(CertificatePaths::SERVER_IDENTITY_PEM),
        },
        host: host.to_owned(),
        port: 0,
        require_client_certificate: true,
    }
}

#[cfg(all(test, feature = "bare-runtime"))]
mod tests {
    use std::path::Path;

    #[test]
    fn requires_a_client_certificate() {
        let config = super::runtime_config(Path::new("."), Path::new("."), "app.appd.local");
        assert!(config.require_client_certificate);
    }
}
