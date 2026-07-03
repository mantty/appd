use std::fs;
use std::process::Command;

use appd_runtime::assets::write_runtime_assets;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn asset_worker_serves_nested_asset_with_manifest_content_type() -> TestResult {
    run_asset_worker_test(
        "asset-worker-test.mjs",
        r#"
import assert from "node:assert/strict";
import worker from "./assets-worker.mjs";

const files = new Map([
  ["/_astro/app.css", { body: "body{color:red}", type: "application/octet-stream" }],
]);

const env = {
  __ASSET_MANIFEST: {
    files: {
      "_astro/app.css": "text/css",
    },
    htmlHandling: "auto-trailing-slash",
    notFoundHandling: "none",
  },
  __ASSET_FILES: {
    async fetch(input, init) {
      const request = new Request(input, init);
      const asset = files.get(new URL(request.url).pathname);
      if (!asset) {
        return new Response("Not Found", { status: 404 });
      }
      return new Response(request.method === "HEAD" ? null : asset.body, {
        status: 200,
        headers: { "content-type": asset.type },
      });
    },
  },
};

const response = await worker.fetch(
  new Request("https://assets.local/_astro/app.css"),
  env,
);

assert.equal(response.status, 200);
assert.equal(response.headers.get("content-type"), "text/css");
assert.equal(await response.text(), "body{color:red}");
"#,
    )
}

#[test]
fn asset_worker_applies_html_and_not_found_routing() -> TestResult {
    run_asset_worker_test(
        "asset-worker-routing-test.mjs",
        r#"
import assert from "node:assert/strict";
import worker from "./assets-worker.mjs";

const files = new Map([
  ["/index.html", "<html>home</html>"],
  ["/about/index.html", "<html>about</html>"],
  ["/404.html", "<html>missing</html>"],
]);

const env = {
  __ASSET_MANIFEST: {
    files: {
      "index.html": "text/html",
      "about/index.html": "text/html",
      "404.html": "text/html",
    },
    htmlHandling: "auto-trailing-slash",
    notFoundHandling: "404-page",
  },
  __ASSET_FILES: {
    async fetch(input, init) {
      const request = new Request(input, init);
      const body = files.get(new URL(request.url).pathname);
      if (body === undefined) {
        return new Response("Not Found", { status: 404 });
      }
      return new Response(request.method === "HEAD" ? null : body, { status: 200 });
    },
  },
};

const about = await worker.fetch(new Request("https://assets.local/about/"), env);
assert.equal(about.status, 200);
assert.equal(await about.text(), "<html>about</html>");

const missing = await worker.fetch(new Request("https://assets.local/nope"), env);
assert.equal(missing.status, 404);
assert.equal(missing.headers.get("content-type"), "text/html");
assert.equal(await missing.text(), "<html>missing</html>");
"#,
    )
}

fn run_asset_worker_test(file_name: &str, script: &str) -> TestResult {
    let temp_dir = tempfile::tempdir()?;
    write_runtime_assets(temp_dir.path())?;

    let test_path = temp_dir.path().join(file_name);
    fs::write(&test_path, script)?;

    let output = Command::new("node").arg(&test_path).output()?;

    assert!(
        output.status.success(),
        "node failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}
