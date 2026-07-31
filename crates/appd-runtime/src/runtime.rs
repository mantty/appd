//! Runtime lifecycle.

use std::path::PathBuf;
use std::sync::Arc;

use appd_bare::{Assets, BareRuntime, Certificates as BareCertificates, RuntimeConfig};
use appd_bundle::AppLayout;

use crate::Result;
use crate::certificates::{Certificates, Renewal};
use crate::events::{Event, Events};

/// What a runtime serves and where it keeps generated state.
#[derive(Clone, Debug)]
pub struct Config {
    /// Packaged application contents.
    pub app: AppLayout,
    /// Writable per-app directory holding generated certificates.
    pub state_dir: PathBuf,
    /// Stable HTTPS host the shell's `WebView` loads.
    pub host: String,
}

/// A running appd application.
///
/// Dropping the runtime stops JavaScript execution and background renewal.
#[derive(Debug)]
pub struct Runtime {
    host: String,
    port: u16,
    certificates: Arc<Certificates>,
    _renewal: Renewal,
    events: Events,
    bare: BareRuntime,
}

impl Runtime {
    /// Start the gateway and JavaScript runtime for a packaged app.
    ///
    /// Blocks until the gateway is listening. `listener` receives every
    /// [`Event`], including those raised from background threads.
    ///
    /// # Errors
    ///
    /// Returns an error when certificates, the packaged bundle, or Bare
    /// startup fail.
    pub fn start(config: Config, listener: impl Fn(Event) + Send + Sync + 'static) -> Result<Self> {
        let events = Events::new(listener);
        events.emit(Event::Starting);
        let certificates = Arc::new(Certificates::start(config.state_dir, config.host.clone())?);
        let bundle = std::fs::read(config.app.worker_bundle())?;
        let bare = BareRuntime::start(
            &bundle,
            &bare_config(&config.app, &certificates, &config.host),
        )?;
        let renewal = certificates.start_renewal(events.clone());
        let port = bare.port();
        events.emit(Event::Listening { port });
        Ok(Self {
            host: config.host,
            port,
            certificates,
            _renewal: renewal,
            events,
            bare,
        })
    }

    /// The host the `WebView` loads.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The loopback port the gateway bound.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Certificate material for the shell's TLS challenge callbacks.
    #[must_use]
    pub fn certificates(&self) -> Arc<Certificates> {
        Arc::clone(&self.certificates)
    }

    /// Suspend JavaScript execution.
    ///
    /// # Errors
    ///
    /// Returns an error when Bare rejects the transition.
    pub fn suspend(&self) -> Result<()> {
        self.bare.suspend(-1)?;
        self.events.emit(Event::Suspended);
        Ok(())
    }

    /// Resume JavaScript execution, renewing certificates that fell due.
    ///
    /// # Errors
    ///
    /// Returns an error when renewal fails or Bare rejects the transition.
    pub fn resume(&self) -> Result<()> {
        self.certificates.refresh()?;
        self.bare.resume()?;
        self.events.emit(Event::Resumed);
        Ok(())
    }
}

fn bare_config(app: &AppLayout, certificates: &Certificates, host: &str) -> RuntimeConfig {
    RuntimeConfig {
        assets: app.serves_assets().then(|| Assets {
            manifest: app.asset_manifest(),
            root: app.assets(),
        }),
        certificates: BareCertificates {
            ca: certificates.authority_path(),
            identity: certificates.identity_path(),
        },
        host: host.to_owned(),
        port: 0,
        require_client_certificate: true,
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, bare_config};
    use crate::certificates::Certificates;
    use appd_bundle::AppLayout;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn always_requires_a_client_certificate() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates =
            Certificates::start(directory.path().to_path_buf(), "app.appd.local".to_owned())?;

        let config = bare_config(
            &AppLayout::new(directory.path()),
            &certificates,
            "app.appd.local",
        );

        assert!(config.require_client_certificate);
        assert_eq!(config.port, 0);
        Ok(())
    }

    #[test]
    fn serves_assets_only_when_the_app_packages_them() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates =
            Certificates::start(directory.path().to_path_buf(), "app.appd.local".to_owned())?;
        let app = AppLayout::new(directory.path());

        assert!(
            bare_config(&app, &certificates, "app.appd.local")
                .assets
                .is_none()
        );

        std::fs::write(app.asset_manifest(), "{}")?;

        assert!(
            bare_config(&app, &certificates, "app.appd.local")
                .assets
                .is_some()
        );
        Ok(())
    }

    #[test]
    fn describes_where_an_app_lives() {
        let config = Config {
            app: AppLayout::new("/apps/example"),
            state_dir: "/state".into(),
            host: "example.appd.local".to_owned(),
        };

        assert_eq!(
            config.app.worker_bundle(),
            std::path::Path::new("/apps/example/worker.bundle")
        );
    }
}
