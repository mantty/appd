//! Runtime JavaScript assets embedded into packaged apps.

use std::fs;
use std::path::Path;

use crate::RuntimeResult;

/// Workerd module that exposes static assets through a Cloudflare-compatible
/// `ASSETS` service binding.
pub const ASSETS_WORKER_JS: &str = include_str!("../assets/assets-worker.mjs");

/// Filename used for the embedded assets service worker module.
pub(crate) const ASSETS_WORKER_FILE_NAME: &str = "assets-worker.mjs";

/// Directory name used for packaged static assets.
pub const PACKAGED_ASSETS_DIR_NAME: &str = "assets";

/// Write runtime JavaScript support modules into a workerd app directory.
///
/// # Errors
///
/// Returns an error when the directory cannot be created or the asset worker
/// cannot be written.
pub fn write_runtime_assets(app_dir: impl AsRef<Path>) -> RuntimeResult<()> {
    let app_dir = app_dir.as_ref();
    fs::create_dir_all(app_dir)?;
    fs::write(app_dir.join(ASSETS_WORKER_FILE_NAME), ASSETS_WORKER_JS)?;
    Ok(())
}
