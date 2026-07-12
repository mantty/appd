use std::fs;
use std::path::Path;

use appd_runtime::{
    RuntimeError,
    wrangler_config::{HtmlHandling, NotFoundHandling, load_config, resolve_config_path},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn load_config_error(path: &Path) -> TestResult<RuntimeError> {
    let Err(error) = load_config(path) else {
        return Err(std::io::Error::other("config should fail").into());
    };
    Ok(error)
}

#[test]
fn discovers_wrangler_config_using_wrangler_order_and_parent_search() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();
    let nested = root.join("apps/web");
    fs::create_dir_all(&nested)?;
    fs::write(root.join("wrangler.toml"), "main = \"toml-entry.mjs\"")?;
    fs::write(
        root.join("wrangler.jsonc"),
        r#"{ "main": "jsonc-entry.mjs" }"#,
    )?;

    let config_path = resolve_config_path(&nested, None)?;

    assert_eq!(config_path, root.join("wrangler.jsonc"));
    Ok(())
}

#[test]
fn discovers_and_parses_json_before_jsonc_and_toml() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();
    let nested = root.join("apps/web");
    fs::create_dir_all(&nested)?;
    fs::write(root.join("wrangler.toml"), "main = \"toml-entry.mjs\"")?;
    fs::write(
        root.join("wrangler.jsonc"),
        r#"{ "main": "jsonc-entry.mjs" }"#,
    )?;
    fs::write(
        root.join("wrangler.json"),
        r#"{ "main": "json-entry.mjs", "compatibility_date": "2026-06-01" }"#,
    )?;

    let config_path = resolve_config_path(&nested, None)?;
    let config = load_config(&config_path)?;

    assert_eq!(config_path, root.join("wrangler.json"));
    assert_eq!(config.main, root.join("json-entry.mjs"));
    Ok(())
}

#[test]
fn explicit_config_path_overrides_default_discovery() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();
    fs::write(root.join("wrangler.jsonc"), r#"{ "main": "ignored.mjs" }"#)?;
    fs::create_dir_all(root.join("config"))?;
    let explicit = root.join("config/custom.toml");
    fs::write(&explicit, "main = \"worker.mjs\"")?;

    let config_path = resolve_config_path(root, Some(Path::new("config/custom.toml")))?;

    assert_eq!(config_path, explicit);
    Ok(())
}

#[test]
fn rejects_missing_required_wrangler_fields() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();
    let config_path = root.join("wrangler.jsonc");
    fs::write(&config_path, r"{}")?;

    let error = load_config_error(&config_path)?;
    assert!(matches!(
        error,
        RuntimeError::MissingWranglerConfigField { field: "main", .. }
    ));

    fs::write(&config_path, r#"{ "main": "worker.mjs", "assets": {} }"#)?;

    let error = load_config_error(&config_path)?;
    assert!(matches!(
        error,
        RuntimeError::MissingWranglerConfigField {
            field: "assets.directory",
            ..
        }
    ));
    Ok(())
}

#[test]
fn rejects_invalid_wrangler_config_values_and_formats() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();

    let unsupported = root.join("wrangler.yaml");
    fs::write(&unsupported, "main: worker.mjs")?;
    assert!(matches!(
        load_config_error(&unsupported)?,
        RuntimeError::UnsupportedWranglerConfigFormat(_)
    ));

    let jsonc_path = root.join("wrangler.jsonc");
    fs::write(&jsonc_path, r#"{ "main": "#)?;
    assert!(matches!(
        load_config_error(&jsonc_path)?,
        RuntimeError::InvalidWranglerConfig { .. }
    ));

    fs::write(
        &jsonc_path,
        r#"{
  "main": "worker.mjs",
  "assets": {
    "directory": "public",
    "html_handling": "surprising"
  }
}"#,
    )?;
    assert!(matches!(
        load_config_error(&jsonc_path)?,
        RuntimeError::InvalidAssetConfig(_)
    ));

    fs::write(
        &jsonc_path,
        r#"{
  "main": "worker.mjs",
  "assets": {
    "directory": "public",
    "not_found_handling": "surprising"
  }
}"#,
    )?;
    assert!(matches!(
        load_config_error(&jsonc_path)?,
        RuntimeError::InvalidAssetConfig(_)
    ));

    fs::write(
        &jsonc_path,
        r#"{ "main": "worker.mjs", "assets": { "directory": "public", "binding": "" } }"#,
    )?;
    assert!(matches!(
        load_config_error(&jsonc_path)?,
        RuntimeError::InvalidAssetConfig(_)
    ));
    Ok(())
}

#[test]
fn parses_jsonc_wrangler_config_subset_and_resolves_paths() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();
    fs::create_dir_all(root.join("build/server"))?;
    fs::create_dir_all(root.join("build/client"))?;
    let config_path = root.join("wrangler.jsonc");
    fs::write(
        &config_path,
        r#"{
  // JSONC comments and trailing commas are valid Wrangler config.
  "main": "build/server/entry.mjs",
  "compatibility_date": "2026-06-01",
  "compatibility_flags": ["nodejs_compat"],
  "assets": {
    "directory": "build/client",
    "binding": "STATIC",
    "html_handling": "drop-trailing-slash",
    "not_found_handling": "single-page-application",
  },
}"#,
    )?;

    let config = load_config(&config_path)?;

    assert_eq!(config.main, root.join("build/server/entry.mjs"));
    let assets = config
        .assets
        .as_ref()
        .ok_or_else(|| std::io::Error::other("assets should be parsed"))?;
    assert_eq!(assets.directory, root.join("build/client"));
    assert_eq!(assets.binding, "STATIC");
    assert_eq!(assets.html_handling, HtmlHandling::Drop);
    assert_eq!(
        assets.not_found_handling,
        NotFoundHandling::SinglePageApplication
    );
    Ok(())
}

#[test]
fn parses_toml_wrangler_config_subset() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();
    let config_path = root.join("wrangler.toml");
    fs::write(
        &config_path,
        r#"
main = "worker/entry.mjs"
compatibility_date = "2026-06-01"
compatibility_flags = ["nodejs_compat"]

[assets]
directory = "public"
binding = "ASSETS"
html_handling = "force-trailing-slash"
not_found_handling = "404-page"
"#,
    )?;

    let config = load_config(&config_path)?;

    assert_eq!(config.main, root.join("worker/entry.mjs"));
    let assets = config
        .assets
        .as_ref()
        .ok_or_else(|| std::io::Error::other("assets should be parsed"))?;
    assert_eq!(assets.directory, root.join("public"));
    assert_eq!(assets.html_handling, HtmlHandling::Force);
    assert_eq!(assets.not_found_handling, NotFoundHandling::Page404);
    Ok(())
}

#[test]
fn parses_exact_asset_paths() -> TestResult {
    assert_eq!(HtmlHandling::parse("none")?, HtmlHandling::None);
    assert_eq!(HtmlHandling::None.as_str(), "none");
    Ok(())
}
