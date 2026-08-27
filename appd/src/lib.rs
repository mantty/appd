#![deny(missing_docs)]

//! The appd runtime.
//!
//! A per-platform shell owns the application and drives this library: it
//! starts a [`Runtime`] for a packaged app, answers platform TLS challenges
//! with [`Certificates`], and reports runtime events.

#[cfg(all(feature = "native", target_os = "android"))]
mod android_jni;
mod app_layout;
#[cfg(feature = "native")]
mod app_service;
#[cfg(all(feature = "native", target_vendor = "apple"))]
mod apple_ffi;
mod asset_manifest;
mod builtins;
#[cfg(feature = "native")]
mod cert_generation;
#[cfg(feature = "native")]
mod cert_storage;
#[cfg(feature = "native")]
mod cert_validation;
#[cfg(feature = "native")]
mod certificates;
#[cfg(feature = "native")]
mod dispatcher;
mod events;
#[cfg(feature = "native")]
mod fs;
#[cfg(feature = "native")]
mod gateway;
mod globals;
#[cfg(feature = "native")]
mod lifecycle_events;
mod network;
mod quickjs;
#[cfg(all(test, feature = "native"))]
mod tests;
#[cfg(feature = "native")]
pub use app_service::{Config, Runtime};
mod streams;
#[cfg(feature = "native")]
mod transport;
mod worker_bundle;
mod worker_compatibility;
mod worker_environment;
mod worker_package_contract;
mod wrangler_config;

use thiserror::Error as Fail;

pub use app_layout::AppLayout;
pub use asset_manifest::write_manifest as write_asset_manifest;
#[cfg(feature = "native")]
pub use certificates::{Certificates, Challenge, Decision};
#[cfg(feature = "native")]
pub use lifecycle_events::Event;
pub use quickjs::Error as QuickJsError;
pub use quickjs::{compile_module, compile_worker};
pub use worker_bundle::{
    WorkerManifest, compress_worker_bundle, compress_worker_module, decompress_worker_bundle,
    decompress_worker_module, read_worker_manifest, write_worker_manifest,
};
pub use worker_compatibility::write_worker_compatibility_sources;
pub use worker_environment::{
    WorkerEnvironment, load as load_worker_environment, write as write_worker_environment,
};
pub use worker_package_contract::{
    Error as WorkerPackageError, Result as WorkerPackageResult, app_host, is_valid_app_name,
};
pub use wrangler_config::{
    HtmlHandling, NotFoundHandling, WranglerAssets, WranglerConfig, WranglerModuleType,
    WranglerRule, load_config as load_wrangler_config,
    resolve_config_path as resolve_wrangler_config_path,
};

/// Runtime result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Runtime failures.
#[derive(Debug, Fail)]
pub enum Error {
    /// Operating-system IO failed.
    #[cfg(feature = "native")]
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Certificate generation failed.
    #[cfg(feature = "native")]
    #[error(transparent)]
    Certificate(#[from] rcgen::Error),
    /// The JavaScript runtime failed to start or change state.
    #[cfg(feature = "native")]
    #[error(transparent)]
    QuickJs(#[from] QuickJsError),
    /// The packaged app contents are invalid.
    #[cfg(feature = "native")]
    #[error(transparent)]
    Bundle(#[from] WorkerPackageError),
    /// Certificate state is held by a thread that stopped unexpectedly.
    #[cfg(feature = "native")]
    #[error("certificate state is unavailable")]
    CertificatesUnavailable,
}

/// Return the URL a shell's `WebView` loads.
#[must_use]
pub fn frontend_url(host: &str) -> String {
    format!("https://{host}/")
}

#[cfg(test)]
#[test]
fn frontend_url_uses_the_stable_host() {
    assert_eq!(frontend_url("app.appd.local"), "https://app.appd.local/");
}
