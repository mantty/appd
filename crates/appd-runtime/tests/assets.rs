use appd_runtime::assets::write_manifest;
use appd_runtime::host::ASSET_DIRECTORY;
use appd_runtime::wrangler_config::{HtmlHandling, NotFoundHandling, WranglerAssets};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn writes_content_types_and_routing_modes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let assets_dir = directory.path().join(ASSET_DIRECTORY);
    std::fs::create_dir_all(assets_dir.join("styles"))?;
    std::fs::write(assets_dir.join("index.html"), "home")?;
    std::fs::write(assets_dir.join("styles/app.css"), "body{}")?;
    let assets = WranglerAssets {
        directory: assets_dir,
        binding: "ASSETS".to_owned(),
        html_handling: HtmlHandling::Drop,
        not_found_handling: NotFoundHandling::SinglePageApplication,
    };

    write_manifest(directory.path(), &assets)?;

    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(
        directory.path().join("asset-manifest.json"),
    )?)?;
    assert_eq!(manifest["binding"], "ASSETS");
    assert_eq!(manifest["files"]["index.html"], "text/html");
    assert_eq!(manifest["files"]["styles/app.css"], "text/css");
    assert_eq!(manifest["htmlHandling"], "drop-trailing-slash");
    assert_eq!(manifest["notFoundHandling"], "single-page-application");
    Ok(())
}
