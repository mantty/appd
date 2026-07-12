//! Static asset manifest generation.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use walkdir::WalkDir;

use crate::host::{ASSET_DIRECTORY, ASSET_MANIFEST_FILE};
use crate::wrangler_config::WranglerAssets;
use crate::{RuntimeError, RuntimeResult};

/// Write the static asset routing manifest for a packaged app.
///
/// # Errors
///
/// Returns an error when asset traversal, path conversion, or writing fails.
pub fn write_manifest(work_dir: &Path, assets: &WranglerAssets) -> RuntimeResult<()> {
    let root = work_dir.join(ASSET_DIRECTORY);
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(&root) {
        let entry = entry.map_err(std::io::Error::other)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&root)
            .map_err(|_| RuntimeError::InvalidUtf8Path(entry.path().to_owned()))?;
        let key = slash_path(relative)?;
        let content_type = mime_guess::from_path(entry.path())
            .first_or_octet_stream()
            .essence_str()
            .to_owned();
        files.insert(key, content_type);
    }
    let manifest = serde_json::json!({
        "binding": assets.binding,
        "files": files,
        "htmlHandling": assets.html_handling.as_str(),
        "notFoundHandling": assets.not_found_handling.as_str(),
    });
    fs::write(
        work_dir.join(ASSET_MANIFEST_FILE),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

fn slash_path(path: &Path) -> RuntimeResult<String> {
    let path = path
        .to_str()
        .ok_or_else(|| RuntimeError::InvalidUtf8Path(path.to_owned()))?;
    Ok(path.replace(std::path::MAIN_SEPARATOR, "/"))
}
