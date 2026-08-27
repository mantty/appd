//! Static asset manifest generation.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use walkdir::WalkDir;

use crate::app_layout::AppLayout;
use crate::worker_package_contract::{Error, Result};
use crate::wrangler_config::WranglerAssets;

/// Write the static asset routing manifest for a packaged app.
///
/// # Errors
///
/// Returns an error when asset traversal, path conversion, or writing fails.
pub fn write_manifest(layout: &AppLayout, assets: &WranglerAssets) -> Result<()> {
    let root = layout.assets();
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(&root) {
        let entry = entry.map_err(std::io::Error::other)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&root)
            .map_err(|_| Error::InvalidUtf8Path(entry.path().to_owned()))?;
        let content_type = mime_guess::from_path(entry.path())
            .first_or_octet_stream()
            .essence_str()
            .to_owned();
        files.insert(slash_path(relative)?, content_type);
    }
    let manifest = serde_json::json!({
        "binding": assets.binding,
        "files": files,
        "htmlHandling": assets.html_handling.as_str(),
        "notFoundHandling": assets.not_found_handling.as_str(),
    });
    fs::write(
        layout.asset_manifest(),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

fn slash_path(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| Error::InvalidUtf8Path(path.to_owned()))?;
    Ok(path.replace(std::path::MAIN_SEPARATOR, "/"))
}
