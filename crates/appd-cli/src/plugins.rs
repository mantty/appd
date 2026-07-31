use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::BuildPlatform;

const MANIFEST: &str = "appd-plugin.json";
const PLATFORMS: [&str; 4] = ["macos", "ios", "ios-simulator", "android"];

#[derive(Clone, Debug)]
pub(crate) struct Plugin {
    pub(crate) id: String,
    root: PathBuf,
    platforms: BTreeMap<String, NativePlatform>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativePlatform {
    pub(crate) class: String,
    #[serde(default)]
    sources: Vec<PathBuf>,
    #[serde(default)]
    pub(crate) frameworks: Vec<String>,
    #[serde(default)]
    pub(crate) plist: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) permissions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginManifest {
    schema_version: u32,
    id: String,
    kind: String,
    #[serde(default)]
    platforms: BTreeMap<String, NativePlatform>,
}

pub(crate) fn discover(project: &Path) -> Result<Vec<Plugin>> {
    let dependencies = dependencies(project)?;
    let mut plugins = Vec::new();
    let mut ids = BTreeSet::new();

    for dependency in dependencies {
        let root = project.join("node_modules").join(&dependency);
        let manifest = root.join(MANIFEST);
        if !manifest.is_file() {
            continue;
        }
        let plugin = load(&root, &manifest)
            .with_context(|| format!("invalid plugin package {dependency}"))?;
        if !ids.insert(plugin.id.clone()) {
            bail!("duplicate appd plugin id '{}'", plugin.id);
        }
        plugins.push(plugin);
    }

    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(plugins)
}

impl Plugin {
    pub(crate) fn platform(&self, platform: BuildPlatform) -> Option<&NativePlatform> {
        self.platforms.get(platform.build_dir_name())
    }

    pub(crate) fn sources(&self, platform: BuildPlatform) -> Result<Vec<PathBuf>> {
        let Some(native) = self.platform(platform) else {
            return Ok(Vec::new());
        };
        let root = fs::canonicalize(&self.root)?;
        native
            .sources
            .iter()
            .map(|source| {
                let path = fs::canonicalize(root.join(source)).with_context(|| {
                    format!(
                        "plugin '{}' source is missing: {}",
                        self.id,
                        source.display()
                    )
                })?;
                if !path.starts_with(&root) {
                    bail!(
                        "plugin '{}' source escapes its package: {}",
                        self.id,
                        source.display()
                    );
                }
                Ok(path)
            })
            .collect()
    }
}

fn dependencies(project: &Path) -> Result<Vec<String>> {
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read(project.join("package.json"))?)?;
    let Some(dependencies) = package
        .get("dependencies")
        .and_then(|value| value.as_object())
    else {
        return Ok(Vec::new());
    };
    Ok(dependencies.keys().cloned().collect())
}

fn load(root: &Path, path: &Path) -> Result<Plugin> {
    let manifest: PluginManifest = serde_json::from_slice(&fs::read(path)?)?;
    if manifest.schema_version != 1 {
        bail!(
            "unsupported plugin schema version {}",
            manifest.schema_version
        );
    }
    if !valid_plugin_id(&manifest.id) {
        bail!(
            "plugin id '{}' must start with a lowercase letter and contain only lowercase letters, digits, and single hyphens",
            manifest.id
        );
    }
    if manifest.kind != "frontend" {
        bail!("plugin '{}' must be a frontend plugin", manifest.id);
    }
    for (platform, native) in &manifest.platforms {
        if !PLATFORMS.contains(&platform.as_str()) {
            bail!("plugin '{}' has unknown platform '{platform}'", manifest.id);
        }
        if !valid_qualified_name(&native.class)
            || (platform == "android" && !native.class.contains('.'))
        {
            bail!(
                "plugin '{}' has invalid {platform} class '{}'",
                manifest.id,
                native.class
            );
        }
        for permission in &native.permissions {
            if !permission.contains('.') || !valid_qualified_name(permission) {
                bail!(
                    "plugin '{}' has invalid Android permission '{permission}'",
                    manifest.id
                );
            }
        }
    }
    Ok(Plugin {
        id: manifest.id,
        root: root.to_path_buf(),
        platforms: manifest.platforms,
    })
}

fn valid_plugin_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 63 || id.starts_with('-') || id.ends_with('-') {
        return false;
    }
    id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !id.contains("--")
}

fn valid_qualified_name(name: &str) -> bool {
    name.split('.').all(|part| {
        let mut bytes = part.bytes();
        bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{discover, valid_plugin_id, valid_qualified_name};
    use crate::BuildPlatform;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn discovers_frontend_plugins_from_dependencies() -> TestResult {
        let root = tempfile::tempdir()?;
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"@appd/geolocation":"1.0.0"}}"#,
        )?;
        let plugin = root.path().join("node_modules/@appd/geolocation");
        fs::create_dir_all(plugin.join("ios"))?;
        fs::write(plugin.join("ios/plugin.swift"), "")?;
        fs::write(
            plugin.join("appd-plugin.json"),
            r#"{
  "schemaVersion": 1,
  "id": "geolocation",
  "kind": "frontend",
  "platforms": {
    "ios": {
      "class": "GeolocationPlugin",
      "sources": ["ios/plugin.swift"],
      "frameworks": ["CoreLocation"]
    }
  }
}"#,
        )?;

        let plugins = discover(root.path())?;

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].id, "geolocation");
        let platform = plugins[0]
            .platform(BuildPlatform::Ios)
            .ok_or("iOS plugin is missing")?;
        assert_eq!(platform.class, "GeolocationPlugin");
        assert_eq!(plugins[0].sources(BuildPlatform::Ios)?.len(), 1);
        Ok(())
    }

    #[test]
    fn rejects_unknown_platforms() -> TestResult {
        let root = tempfile::tempdir()?;
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"plugin":"1.0.0"}}"#,
        )?;
        let plugin = root.path().join("node_modules/plugin");
        fs::create_dir_all(&plugin)?;
        fs::write(
            plugin.join("appd-plugin.json"),
            r#"{
  "schemaVersion": 1,
  "id": "bad",
  "kind": "frontend",
  "platforms": {"dreamcast": {"class": "Bad"}}
}"#,
        )?;

        let Err(error) = discover(root.path()) else {
            return Err("unknown platform was accepted".into());
        };

        assert!(format!("{error:#}").contains("unknown platform 'dreamcast'"));
        Ok(())
    }

    #[test]
    fn rejects_web_as_native_platform_metadata() -> TestResult {
        let root = tempfile::tempdir()?;
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"plugin":"1.0.0"}}"#,
        )?;
        let plugin = root.path().join("node_modules/plugin");
        fs::create_dir_all(&plugin)?;
        fs::write(
            plugin.join("appd-plugin.json"),
            r#"{
  "schemaVersion": 1,
  "id": "bad",
  "kind": "frontend",
  "platforms": {"web": {"class": "Bad"}}
}"#,
        )?;

        let Err(error) = discover(root.path()) else {
            return Err("web native metadata was accepted".into());
        };

        assert!(format!("{error:#}").contains("unknown platform 'web'"));
        Ok(())
    }

    #[test]
    fn validates_protocol_identifiers() {
        assert!(valid_plugin_id("geolocation"));
        assert!(valid_plugin_id("photo-library2"));
        assert!(!valid_plugin_id("PhotoLibrary"));
        assert!(!valid_plugin_id("2photo"));
        assert!(!valid_plugin_id("photo--library"));
        assert!(!valid_plugin_id("../photo"));

        assert!(valid_qualified_name("AppdGeolocationPlugin"));
        assert!(valid_qualified_name(
            "com.appd.plugins.geolocation.AppdGeolocationPlugin"
        ));
        assert!(!valid_qualified_name("com.appd.Location-Plugin"));
        assert!(!valid_qualified_name("com.appd.2Location"));
    }
}
