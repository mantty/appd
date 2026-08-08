//! Minimal Wrangler configuration loading for appd packaging.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use jsonc_parser::{ParseOptions, parse_to_serde_value};
use serde::Deserialize;
use serde_json::Value;

use crate::{Error, Result};

const CONFIG_FILE_NAMES: [&str; 3] = ["wrangler.json", "wrangler.jsonc", "wrangler.toml"];

/// Resolved subset of a Wrangler config that appd consumes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WranglerConfig {
    /// Absolute path to the config file that was parsed.
    pub path: PathBuf,
    /// Worker entrypoint, resolved relative to the config file directory.
    pub main: PathBuf,
    /// Static asset configuration, when the Worker declares assets.
    pub assets: Option<WranglerAssets>,
    /// Text and JSON environment bindings declared in `vars`.
    pub vars: BTreeMap<String, Value>,
}

/// Static asset subset of a Wrangler config that appd consumes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WranglerAssets {
    /// Static asset directory, resolved relative to the config file directory.
    pub directory: PathBuf,
    /// Worker binding name. Defaults to `ASSETS`.
    pub binding: String,
    /// Cloudflare-style HTML path handling mode.
    pub html_handling: HtmlHandling,
    /// Cloudflare-style asset miss handling mode.
    pub not_found_handling: NotFoundHandling,
}

/// Cloudflare static asset `html_handling` mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlHandling {
    /// Match asset paths exactly.
    None,
    /// Use Cloudflare's automatic trailing-slash behavior.
    Auto,
    /// Prefer directory-index paths.
    Force,
    /// Prefer extension paths.
    Drop,
}

impl HtmlHandling {
    /// Parse a Wrangler `assets.html_handling` value.
    ///
    /// # Errors
    ///
    /// Returns an error for values appd does not support.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "none" => Ok(Self::None),
            "auto-trailing-slash" => Ok(Self::Auto),
            "force-trailing-slash" => Ok(Self::Force),
            "drop-trailing-slash" => Ok(Self::Drop),
            _ => Err(Error::InvalidAssetConfig(format!(
                "unsupported assets.html_handling value '{value}'"
            ))),
        }
    }

    /// Return the Wrangler string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Auto => "auto-trailing-slash",
            Self::Force => "force-trailing-slash",
            Self::Drop => "drop-trailing-slash",
        }
    }
}

/// Cloudflare static asset `not_found_handling` mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotFoundHandling {
    /// Return a plain 404 when no asset matches.
    None,
    /// Serve `/index.html` with status 200 when no asset matches.
    SinglePageApplication,
    /// Serve the nearest `404.html` with status 404 when no asset matches.
    Page404,
}

impl NotFoundHandling {
    /// Parse a Wrangler `assets.not_found_handling` value.
    ///
    /// # Errors
    ///
    /// Returns an error for values appd does not support.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "none" => Ok(Self::None),
            "single-page-application" => Ok(Self::SinglePageApplication),
            "404-page" => Ok(Self::Page404),
            _ => Err(Error::InvalidAssetConfig(format!(
                "unsupported assets.not_found_handling value '{value}'"
            ))),
        }
    }

    /// Return the Wrangler string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SinglePageApplication => "single-page-application",
            Self::Page404 => "404-page",
        }
    }
}

/// Resolve the Wrangler config path to use.
///
/// When `explicit_config` is provided, it is resolved relative to
/// `reference_dir` unless already absolute. Without an explicit path, appd
/// mirrors Wrangler's file order and parent-directory search:
/// `wrangler.json`, then `wrangler.jsonc`, then `wrangler.toml`.
///
/// # Errors
///
/// Returns an error if no config file can be found.
pub fn resolve_config_path(
    reference_dir: &Path,
    explicit_config: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(path) = explicit_config {
        return Ok(resolve_path(reference_dir, path));
    }

    for file_name in CONFIG_FILE_NAMES {
        if let Some(path) = find_file_upwards(reference_dir, file_name) {
            return Ok(path);
        }
    }

    Err(Error::ConfigNotFound(reference_dir.to_path_buf()))
}

/// Load a Wrangler configuration file.
///
/// # Errors
///
/// Returns an error when the file cannot be read, cannot be parsed, uses an
/// unsupported format, or omits a field appd needs to package a Worker.
pub fn load_config(config_path: &Path) -> Result<WranglerConfig> {
    let config_path = absolute_path(config_path)?;
    let raw = parse_config(&config_path)?;
    let config_dir = config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let main = raw.main.ok_or_else(|| Error::MissingConfigField {
        path: config_path.clone(),
        field: "main",
    })?;

    Ok(WranglerConfig {
        path: config_path.clone(),
        main: resolve_path(&config_dir, Path::new(&main)),
        assets: raw
            .assets
            .map(|assets| resolve_assets(&config_path, &config_dir, assets))
            .transpose()?,
        vars: raw.vars,
    })
}

#[derive(Debug, Deserialize)]
struct RawWranglerConfig {
    main: Option<String>,
    assets: Option<RawWranglerAssets>,
    #[serde(default)]
    vars: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct RawWranglerAssets {
    directory: Option<String>,
    binding: Option<String>,
    html_handling: Option<String>,
    not_found_handling: Option<String>,
}

fn resolve_assets(
    config_path: &Path,
    config_dir: &Path,
    assets: RawWranglerAssets,
) -> Result<WranglerAssets> {
    let directory = assets.directory.ok_or_else(|| Error::MissingConfigField {
        path: config_path.to_path_buf(),
        field: "assets.directory",
    })?;
    let binding = assets.binding.unwrap_or_else(|| "ASSETS".to_owned());
    if binding.is_empty() {
        return Err(Error::InvalidAssetConfig(
            "assets.binding must not be empty".to_owned(),
        ));
    }

    Ok(WranglerAssets {
        directory: resolve_path(config_dir, Path::new(&directory)),
        binding,
        html_handling: assets
            .html_handling
            .as_deref()
            .map(HtmlHandling::parse)
            .transpose()?
            .unwrap_or(HtmlHandling::Auto),
        not_found_handling: assets
            .not_found_handling
            .as_deref()
            .map(NotFoundHandling::parse)
            .transpose()?
            .unwrap_or(NotFoundHandling::None),
    })
}

fn parse_config(config_path: &Path) -> Result<RawWranglerConfig> {
    let content = fs::read_to_string(config_path)?;
    let extension = config_path
        .extension()
        .and_then(|extension| extension.to_str());

    match extension {
        Some("json" | "jsonc") => parse_jsonc_config(config_path, &content),
        Some("toml") => parse_toml_config(config_path, &content),
        _ => Err(Error::UnsupportedConfigFormat(config_path.to_path_buf())),
    }
}

fn parse_jsonc_config(config_path: &Path, content: &str) -> Result<RawWranglerConfig> {
    parse_to_serde_value(content, &ParseOptions::default()).map_err(|error| Error::InvalidConfig {
        path: config_path.to_path_buf(),
        message: error.to_string(),
    })
}

fn parse_toml_config(config_path: &Path, content: &str) -> Result<RawWranglerConfig> {
    toml::from_str(content).map_err(|error| Error::InvalidConfig {
        path: config_path.to_path_buf(),
        message: error.to_string(),
    })
}

fn find_file_upwards(start: &Path, file_name: &str) -> Option<PathBuf> {
    let mut dir = absolute_path(start).ok()?;
    loop {
        let candidate = dir.join(file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn resolve_path(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}
