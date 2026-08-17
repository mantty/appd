//! Certificate lifecycle and platform trust decisions.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use time::OffsetDateTime;

use crate::events::{Event, Events};
use crate::material::{CertificateBundle, CertificatePaths};
use crate::{Error, Result};

const RETRY_DELAY: Duration = Duration::from_hours(1);

type Shared = Arc<RwLock<CertificateBundle>>;

/// A TLS authentication challenge raised by a platform `WebView`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Challenge<'a> {
    /// The platform is deciding whether to trust the gateway's certificate.
    ServerTrust {
        /// Host the connection was made to.
        host: &'a str,
    },
    /// The gateway asked the platform for a client certificate.
    ClientCertificate {
        /// Host the connection was made to.
        host: &'a str,
        /// How many times this challenge has already failed.
        previous_failures: usize,
    },
}

/// How a shell answers a [`Challenge`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    /// Not an appd connection. Let the platform decide.
    PerformDefault,
    /// An appd connection appd cannot authenticate. Refuse it.
    Cancel,
    /// Trust the certificate only where it chains to this authority, in DER.
    TrustAuthority(Vec<u8>),
    /// Present this client identity.
    PresentIdentity {
        /// Client certificate, in DER.
        certificate: Vec<u8>,
        /// Client private key, in PKCS#8 DER.
        private_key: Vec<u8>,
    },
}

/// Certificate material for the app's local mTLS gateway.
///
/// Certificates are generated on first use, cached in the app's state
/// directory, and renewed in the background before they expire.
#[derive(Debug)]
pub struct Certificates {
    state_dir: PathBuf,
    host: String,
    current: Shared,
}

impl Certificates {
    pub(crate) fn start(state_dir: PathBuf, host: String) -> Result<Self> {
        let bundle = CertificateBundle::ensure(&state_dir, &host, OffsetDateTime::now_utc())?;
        let current: Shared = Arc::new(RwLock::new(bundle));
        Ok(Self {
            state_dir,
            host,
            current,
        })
    }

    pub(crate) fn start_renewal(self: &Arc<Self>, events: Events) -> Renewal {
        Renewal::start(Arc::clone(self), events)
    }

    /// Renew any certificate that is due.
    pub(crate) fn refresh(&self) -> Result<()> {
        renew(&self.state_dir, &self.host, &self.current)
    }

    /// The authority the gateway trusts for client certificates.
    pub(crate) fn authority_path(&self) -> PathBuf {
        self.state_dir.join(CertificatePaths::CA_CERT_PEM)
    }

    /// The certificate and key the gateway serves.
    pub(crate) fn server_certificate_path(&self) -> PathBuf {
        self.state_dir.join(CertificatePaths::SERVER_CERT_PEM)
    }

    pub(crate) fn server_key_path(&self) -> PathBuf {
        self.state_dir.join(CertificatePaths::SERVER_KEY_PEM)
    }

    /// Decide how a shell should answer a platform TLS challenge.
    #[must_use]
    pub fn decide(&self, challenge: &Challenge<'_>) -> Decision {
        let (Challenge::ServerTrust { host } | Challenge::ClientCertificate { host, .. }) =
            *challenge;
        if !same_dns_host(host, &self.host) {
            return Decision::PerformDefault;
        }
        let Ok(current) = self.current.read() else {
            return Decision::Cancel;
        };
        match *challenge {
            Challenge::ServerTrust { .. } => Decision::TrustAuthority(current.ca_cert_der.clone()),
            Challenge::ClientCertificate {
                previous_failures, ..
            } => client_identity(&current, previous_failures),
        }
    }

    /// Return whether `certificate` is the current server certificate for `host`.
    #[must_use]
    pub fn trusts_server_certificate(&self, host: &str, certificate: &str) -> bool {
        if !same_dns_host(host, &self.host) {
            return false;
        }
        let Ok(current) = self.current.read() else {
            return false;
        };
        crate::material::certificate_der(certificate)
            == crate::material::certificate_der(&current.server_cert_pem)
    }
}

fn client_identity(current: &CertificateBundle, previous_failures: usize) -> Decision {
    if previous_failures > 0 {
        return Decision::Cancel;
    }
    current
        .client_certificate_der()
        .map_or(Decision::Cancel, |certificate| Decision::PresentIdentity {
            certificate,
            private_key: current.client_key_der.clone(),
        })
}

fn same_dns_host(left: &str, right: &str) -> bool {
    left.trim_end_matches('.')
        .eq_ignore_ascii_case(right.trim_end_matches('.'))
}

#[derive(Debug)]
pub(crate) struct Renewal {
    stop: Sender<()>,
    task: Option<JoinHandle<()>>,
}

impl Renewal {
    fn start(certificates: Arc<Certificates>, events: Events) -> Self {
        let (stop, stopped) = mpsc::channel();
        let task = thread::spawn(move || {
            let mut retrying = false;
            loop {
                let delay = if retrying {
                    RETRY_DELAY
                } else {
                    certificates.next_delay()
                };
                if stopped.recv_timeout(delay).is_ok() {
                    break;
                }
                match certificates.refresh() {
                    Ok(()) => {
                        retrying = false;
                        events.emit(Event::CertificatesRenewed);
                    }
                    Err(error) => {
                        retrying = true;
                        events.emit(Event::Failed {
                            message: format!("certificate renewal failed: {error}"),
                        });
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

impl Drop for Renewal {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}

fn renew(state_dir: &Path, host: &str, current: &Shared) -> Result<()> {
    let bundle = CertificateBundle::ensure(state_dir, host, OffsetDateTime::now_utc())?;
    let mut held = current
        .write()
        .map_err(|_| Error::CertificatesUnavailable)?;
    *held = bundle;
    Ok(())
}

impl Certificates {
    fn next_delay(&self) -> Duration {
        self.current.read().map_or(RETRY_DELAY, |held| {
            held.renewal_delay(OffsetDateTime::now_utc())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Certificates, Challenge, Decision};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn certificates(directory: &std::path::Path) -> TestResult<Certificates> {
        Ok(Certificates::start(
            directory.to_path_buf(),
            "app.appd.local".to_owned(),
        )?)
    }

    #[test]
    fn pins_the_generated_authority_for_the_app_host() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates = certificates(directory.path())?;

        let decision = certificates.decide(&Challenge::ServerTrust {
            host: "APP.APPD.LOCAL.",
        });

        assert!(matches!(decision, Decision::TrustAuthority(der) if !der.is_empty()));
        Ok(())
    }

    #[test]
    fn presents_a_client_identity_for_the_app_host() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates = certificates(directory.path())?;

        let decision = certificates.decide(&Challenge::ClientCertificate {
            host: "app.appd.local",
            previous_failures: 0,
        });

        let Decision::PresentIdentity {
            certificate,
            private_key,
        } = decision
        else {
            return Err(std::io::Error::other("expected a client identity").into());
        };
        assert!(!certificate.is_empty());
        assert!(!private_key.is_empty());
        Ok(())
    }

    #[test]
    fn leaves_other_hosts_to_the_platform() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates = certificates(directory.path())?;

        assert_eq!(
            certificates.decide(&Challenge::ServerTrust {
                host: "example.com"
            }),
            Decision::PerformDefault
        );
        assert_eq!(
            certificates.decide(&Challenge::ClientCertificate {
                host: "example.com",
                previous_failures: 0,
            }),
            Decision::PerformDefault
        );
        Ok(())
    }

    #[test]
    fn refuses_a_client_certificate_that_already_failed() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates = certificates(directory.path())?;

        assert_eq!(
            certificates.decide(&Challenge::ClientCertificate {
                host: "app.appd.local",
                previous_failures: 1,
            }),
            Decision::Cancel
        );
        Ok(())
    }

    #[test]
    fn trusts_only_the_current_server_certificate_for_the_app_host() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates = certificates(directory.path())?;
        let server = std::fs::read_to_string(certificates.server_certificate_path())?;
        let certificate = server
            .split("-----END CERTIFICATE-----")
            .next()
            .ok_or("server certificate is missing")?;
        let certificate = format!("{certificate}-----END CERTIFICATE-----\n");

        assert!(certificates.trusts_server_certificate("app.appd.local", &certificate));
        assert!(!certificates.trusts_server_certificate("example.com", &certificate));
        assert!(!certificates.trusts_server_certificate("app.appd.local", "invalid"));
        Ok(())
    }

    #[test]
    fn serves_the_gateway_from_cached_paths() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates = certificates(directory.path())?;

        assert!(certificates.authority_path().is_file());
        assert!(certificates.server_certificate_path().is_file());
        assert!(certificates.server_key_path().is_file());
        certificates.refresh()?;
        assert!(certificates.server_certificate_path().is_file());
        assert!(certificates.server_key_path().is_file());
        Ok(())
    }
}
