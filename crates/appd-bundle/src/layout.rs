//! Paths inside a packaged app directory.

use std::path::{Path, PathBuf};

const WORKER_BUNDLE: &str = "worker.bundle";
const ASSET_MANIFEST: &str = "asset-manifest.json";
const ASSETS: &str = "assets";

/// The packaged contents of an appd application.
///
/// `appd-cli` writes this layout and the runtime reads it. Both ask for paths
/// rather than naming files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppLayout {
    root: PathBuf,
}

impl AppLayout {
    /// Describe the layout rooted at a packaged app directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The packaged app directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The Bare worker bundle.
    #[must_use]
    pub fn worker_bundle(&self) -> PathBuf {
        self.root.join(WORKER_BUNDLE)
    }

    /// The static asset routing manifest.
    #[must_use]
    pub fn asset_manifest(&self) -> PathBuf {
        self.root.join(ASSET_MANIFEST)
    }

    /// The static asset directory.
    #[must_use]
    pub fn assets(&self) -> PathBuf {
        self.root.join(ASSETS)
    }

    /// Whether the packaged app serves static assets.
    #[must_use]
    pub fn serves_assets(&self) -> bool {
        self.asset_manifest().is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::AppLayout;

    #[test]
    fn resolves_every_path_under_the_app_root() {
        let layout = AppLayout::new("/apps/example");
        assert_eq!(
            layout.worker_bundle(),
            std::path::Path::new("/apps/example/worker.bundle")
        );
        assert_eq!(
            layout.asset_manifest(),
            std::path::Path::new("/apps/example/asset-manifest.json")
        );
        assert_eq!(
            layout.assets(),
            std::path::Path::new("/apps/example/assets")
        );
    }

    #[test]
    fn reports_no_assets_without_a_manifest() {
        assert!(!AppLayout::new("/apps/missing").serves_assets());
    }
}
