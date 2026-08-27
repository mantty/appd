//! Application lifecycle and packaged-worker startup.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app_layout::AppLayout;
use crate::certificates::{Certificates, Renewal};
use crate::lifecycle_events::{Event, Events};
use crate::quickjs::{
    Assets, Certificates as QuickJsCertificates, QuickJsRuntime, RuntimeConfig, WorkerBundle,
};
use crate::worker_bundle::{decompress_worker_bundle, read_worker_manifest};
use crate::worker_environment::load as load_environment;

use crate::Result;

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
    certificates: Arc<Certificates>,
    _renewal: Renewal,
    events: Events,
    quickjs: QuickJsRuntime,
}

impl Runtime {
    /// Start the gateway and JavaScript runtime for a packaged app.
    ///
    /// Blocks until the gateway is listening. `listener` receives every
    /// [`Event`], including those raised from background threads.
    ///
    /// # Errors
    ///
    /// Returns an error when certificates, the packaged bundle, or `QuickJS`
    /// startup fail.
    pub fn start(config: Config, listener: impl Fn(Event) + Send + Sync + 'static) -> Result<Self> {
        let events = Events::new(listener);
        events.emit(Event::Starting);
        let state_dir = config.state_dir.clone();
        let certificates = Arc::new(Certificates::start(state_dir.clone(), config.host.clone())?);
        let worker = packaged_worker(&config.app)?;
        let quickjs = QuickJsRuntime::start(
            worker,
            &quickjs_config(&config.app, &certificates, &config.host, &state_dir)?,
        )?;
        let renewal = certificates.start_renewal(events.clone());
        events.emit(Event::Listening {
            port: quickjs.port(),
        });
        Ok(Self {
            host: config.host,
            certificates,
            _renewal: renewal,
            events,
            quickjs,
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
        self.quickjs.port()
    }

    /// Wait for the loopback gateway to respond and return its current port.
    ///
    /// # Errors
    ///
    /// Returns an error when the gateway does not recover within its timeout.
    pub fn restore_gateway(&self) -> Result<u16> {
        Ok(self.quickjs.restore_gateway()?)
    }

    /// Certificate material for the shell's TLS challenge callbacks.
    #[must_use]
    pub fn certificates(&self) -> Arc<Certificates> {
        Arc::clone(&self.certificates)
    }

    /// Stop new JavaScript dispatch and quiesce gateway connections.
    ///
    /// # Errors
    ///
    /// Returns an error when `QuickJS` rejects the transition.
    pub fn suspend(&self) -> Result<()> {
        self.quickjs.suspend();
        self.events.emit(Event::Suspended);
        Ok(())
    }

    /// Resume JavaScript execution, renewing certificates that fell due.
    ///
    /// # Errors
    ///
    /// Returns an error when renewal fails or `QuickJS` rejects the transition.
    pub fn resume(&self) -> Result<()> {
        self.certificates.refresh()?;
        self.quickjs.resume();
        self.events.emit(Event::Resumed);
        Ok(())
    }
}

fn packaged_worker(app: &AppLayout) -> Result<WorkerBundle> {
    if app.worker_manifest().is_file() {
        let manifest = read_worker_manifest(app)?;
        Ok(WorkerBundle::from_modules(
            manifest.entry,
            app.worker_modules(),
            app.bundle(),
        ))
    } else {
        let bytecode = decompress_worker_bundle(&std::fs::read(app.worker_bundle())?)?;
        Ok(WorkerBundle::from_bytecode(bytecode, app.bundle()))
    }
}

fn quickjs_config(
    app: &AppLayout,
    certificates: &Certificates,
    host: &str,
    state_dir: &Path,
) -> Result<RuntimeConfig> {
    Ok(RuntimeConfig {
        assets: app.serves_assets().then(|| Assets {
            manifest: app.asset_manifest(),
            root: app.assets(),
        }),
        cache: state_dir.join("cache"),
        certificates: QuickJsCertificates {
            ca: certificates.authority_path(),
            certificate: certificates.server_certificate_path(),
            private_key: certificates.server_key_path(),
        },
        environment: load_environment(app)?.vars,
        host: host.to_owned(),
        port: 0,
        require_client_certificate: true,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::worker_environment::{WorkerEnvironment, write as write_environment};
    use serde_json::json;

    use super::{Config, quickjs_config};
    use crate::app_layout::AppLayout;
    use crate::certificates::Certificates;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn always_requires_a_client_certificate() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates =
            Certificates::start(directory.path().to_path_buf(), "app.appd.local".to_owned())?;
        let app = AppLayout::new(directory.path());
        write_environment(&app, &WorkerEnvironment::default())?;

        let config = quickjs_config(&app, &certificates, "app.appd.local", directory.path())?;

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
        write_environment(&app, &WorkerEnvironment::default())?;

        assert!(
            quickjs_config(&app, &certificates, "app.appd.local", directory.path())?
                .assets
                .is_none()
        );

        std::fs::write(app.asset_manifest(), "{}")?;

        assert!(
            quickjs_config(&app, &certificates, "app.appd.local", directory.path())?
                .assets
                .is_some()
        );
        Ok(())
    }

    #[test]
    fn loads_packaged_worker_vars() -> TestResult {
        let directory = tempfile::tempdir()?;
        let certificates =
            Certificates::start(directory.path().to_path_buf(), "app.appd.local".to_owned())?;
        let app = AppLayout::new(directory.path());
        write_environment(
            &app,
            &WorkerEnvironment {
                vars: BTreeMap::from([("JSON".to_owned(), json!({ "enabled": true }))]),
            },
        )?;

        assert_eq!(
            quickjs_config(&app, &certificates, "app.appd.local", directory.path())?
                .environment
                .get("JSON"),
            Some(&json!({ "enabled": true }))
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
