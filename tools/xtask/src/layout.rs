use std::env;
use std::path::{Path, PathBuf};

use appd_cli::{MANIFEST_FILE, Target};

pub(crate) struct WorkspaceLayout {
    root: PathBuf,
}

impl WorkspaceLayout {
    pub(crate) fn from_source() -> Option<Self> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir.parent()?.parent()?.to_path_buf();
        let layout = Self { root };
        (layout.root.join("Cargo.toml").is_file() && layout.root.join("appd").is_dir())
            .then_some(layout)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn target_pack(&self, target: Target) -> PathBuf {
        self.root
            .join("target/appd-target-packs")
            .join(target.to_string())
    }

    pub(crate) fn recipe_output(&self, target: Target) -> PathBuf {
        self.root
            .join("target/appd-target-pack-staging")
            .join(target.to_string())
    }

    pub(crate) fn platform_recipe(&self, target: Target) -> PathBuf {
        let platform = target.platform();
        self.root
            .join("platforms")
            .join(platform.repository_directory_name())
            .join("build")
            .join(platform.target_pack_recipe_file_name())
    }

    pub(crate) fn esbuild(&self) -> PathBuf {
        self.root.join("plugins/node_modules/esbuild")
    }

    #[cfg(test)]
    pub(crate) fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub(crate) fn manifest(&self, target: Target) -> PathBuf {
        self.target_pack(target).join(MANIFEST_FILE)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::WorkspaceLayout;
    use appd_cli::Target;

    #[test]
    fn resolves_workspace_artifacts_from_one_root() {
        let layout = WorkspaceLayout {
            root: "/workspace/appd".into(),
        };

        assert_eq!(
            layout.target_pack(Target::MacosArm64),
            Path::new("/workspace/appd/target/appd-target-packs/macos-arm64")
        );
        assert_eq!(
            layout.platform_recipe(Target::AndroidArm64),
            Path::new("/workspace/appd/platforms/android/build/target-pack")
        );
        assert_eq!(
            layout.platform_recipe(Target::IosArm64),
            Path::new("/workspace/appd/platforms/apple/build/target-pack")
        );
        assert_eq!(
            layout.platform_recipe(Target::WindowsX64),
            Path::new("/workspace/appd/platforms/windows/build/target-pack.ps1")
        );
        assert_eq!(
            layout.esbuild(),
            Path::new("/workspace/appd/plugins/node_modules/esbuild")
        );
    }
}
