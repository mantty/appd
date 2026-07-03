use std::fs;

use appd_runtime::workerd_config::{ConfigOptions, generate};
use appd_runtime::wrangler_config::load_config;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn generates_workerd_config_for_astro_worker_assets_and_mtls() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();

    fs::create_dir_all(root.join("worker/chunks"))?;
    fs::create_dir_all(root.join("assets/_astro"))?;
    fs::write(root.join("worker/entry.mjs"), "export default {};")?;
    fs::write(root.join("worker/chunks/page.mjs"), "export {};")?;
    fs::write(root.join("worker/readme.txt"), "ignored")?;
    fs::write(root.join("assets/index.html"), "<html></html>")?;
    fs::write(root.join("assets/_astro/app.js"), "console.log('app');")?;
    fs::write(
        root.join("wrangler.jsonc"),
        r#"{
  "main": "worker/entry.mjs",
  "compatibility_flags": ["nodejs_compat"],
  "assets": {
    "directory": "assets"
  }
}"#,
    )?;

    let config = generate(&ConfigOptions {
        work_dir: root.to_path_buf(),
        worker_dir: "worker".into(),
        worker_main_module: "entry.mjs".to_owned(),
        wrangler_config: load_config(&root.join("wrangler.jsonc"))?,
        jitless: true,
    })?;

    assert!(config.contains("requireClientCerts = true"));
    assert!(config.contains("trustedCertificates = [embed \"ca.cert.pem\"]"));
    assert!(config.contains("v8Flags = [\"--jitless\"]"));
    assert!(config.contains("(name = \"entry.mjs\", esModule = embed \"worker/entry.mjs\")"));
    assert!(
        config
            .contains("(name = \"chunks/page.mjs\", esModule = embed \"worker/chunks/page.mjs\")")
    );
    assert!(!config.contains("readme.txt"));
    assert!(config.contains("(name = \"assets-disk\", disk = \"assets\")"));
    assert!(config.contains("(name = \"__ASSET_FILES\", service = \"assets-disk\")"));
    assert!(config.contains("(name = \"__ASSET_MANIFEST\", json = \""));
    assert!(config.contains("\\\"files\\\":{"));
    assert!(config.contains("\\\"index.html\\\":\\\"text/html\\\""));
    assert!(config.contains("\\\"_astro/app.js\\\":\\\"text/javascript\\\""));
    assert!(config.contains("\\\"htmlHandling\\\":\\\"auto-trailing-slash\\\""));
    assert!(config.contains("\\\"notFoundHandling\\\":\\\"none\\\""));
    assert!(!config.contains("kvNamespace"));
    assert!(!config.contains("@cloudflare/kv-asset-handler"));

    let written = fs::read_to_string(root.join("config.capnp"))?;
    assert_eq!(written, config);

    Ok(())
}

#[test]
fn omits_jitless_flag_when_target_allows_jit() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();

    fs::create_dir_all(root.join("worker"))?;
    fs::write(root.join("worker/entry.mjs"), "export default {};")?;
    fs::write(
        root.join("wrangler.jsonc"),
        r#"{
  "main": "worker/entry.mjs"
}"#,
    )?;

    let config = generate(&ConfigOptions {
        work_dir: root.to_path_buf(),
        worker_dir: "worker".into(),
        worker_main_module: "entry.mjs".to_owned(),
        wrangler_config: load_config(&root.join("wrangler.jsonc"))?,
        jitless: false,
    })?;

    assert!(config.contains("v8Flags = []"));
    assert!(!config.contains("--jitless"));

    Ok(())
}

#[test]
fn reads_asset_routing_options_from_wrangler_config() -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    let root = temp_dir.path();

    fs::create_dir_all(root.join("worker"))?;
    fs::create_dir_all(root.join("assets"))?;
    fs::write(root.join("worker/entry.mjs"), "export default {};")?;
    fs::write(root.join("assets/index.html"), "<html></html>")?;
    fs::write(
        root.join("wrangler.jsonc"),
        r#"{
  "main": "worker/entry.mjs",
  "assets": {
    "directory": "assets",
    "binding": "STATIC",
    "not_found_handling": "single-page-application",
    "html_handling": "drop-trailing-slash"
  }
}"#,
    )?;

    let config = generate(&ConfigOptions {
        work_dir: root.to_path_buf(),
        worker_dir: "worker".into(),
        worker_main_module: "entry.mjs".to_owned(),
        wrangler_config: load_config(&root.join("wrangler.jsonc"))?,
        jitless: false,
    })?;

    assert!(config.contains("(name = \"STATIC\", service = \"assets\")"));
    assert!(config.contains("\\\"notFoundHandling\\\":\\\"single-page-application\\\""));
    assert!(config.contains("\\\"htmlHandling\\\":\\\"drop-trailing-slash\\\""));

    Ok(())
}
