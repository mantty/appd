use appd::{AppLayout, HtmlHandling, NotFoundHandling, WranglerAssets, write_asset_manifest};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn writes_content_types_and_routing_modes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let layout = AppLayout::new(directory.path());
    std::fs::create_dir_all(layout.assets().join("styles"))?;
    std::fs::write(layout.assets().join("index.html"), "home")?;
    std::fs::write(layout.assets().join("styles/app.css"), "body{}")?;
    let assets = WranglerAssets {
        directory: layout.assets(),
        binding: "ASSETS".to_owned(),
        html_handling: HtmlHandling::Drop,
        not_found_handling: NotFoundHandling::SinglePageApplication,
    };

    write_asset_manifest(&layout, &assets)?;

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(layout.asset_manifest())?)?;
    assert_eq!(manifest["binding"], "ASSETS");
    assert_eq!(manifest["files"]["index.html"], "text/html");
    assert_eq!(manifest["files"]["styles/app.css"], "text/css");
    assert_eq!(manifest["htmlHandling"], "drop-trailing-slash");
    assert_eq!(manifest["notFoundHandling"], "single-page-application");
    assert!(layout.serves_assets());
    Ok(())
}
