//! `workerd` configuration generation for packaged `appd` applications.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::assets::{ASSETS_WORKER_FILE_NAME, PACKAGED_ASSETS_DIR_NAME};
use crate::wrangler_config::{WranglerAssets, WranglerConfig};
use crate::{RuntimeError, RuntimeResult};

/// Options used to generate `config.capnp`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigOptions {
    /// App work directory containing packaged Worker and asset files.
    pub work_dir: PathBuf,
    /// Path to the packaged Worker modules relative to [`Self::work_dir`].
    pub worker_dir: PathBuf,
    /// Name of the Worker entry module relative to [`Self::worker_dir`].
    pub worker_main_module: String,
    /// Parsed Wrangler configuration subset used by appd.
    pub wrangler_config: WranglerConfig,
    /// Whether to force V8 `--jitless`.
    pub jitless: bool,
}

/// Generate and write `config.capnp`, returning the generated content.
///
/// # Errors
///
/// Returns an error when module scanning, JSON manifest generation, path
/// conversion, or file writing fails.
pub fn generate(options: &ConfigOptions) -> RuntimeResult<String> {
    let modules = scan_modules(
        &options.work_dir,
        &options.worker_dir,
        &options.worker_main_module,
    )?;
    let asset_manifest =
        build_asset_manifest(&options.work_dir, options.wrangler_config.assets.as_ref())?;
    let config = render_config(options, &modules, asset_manifest.as_deref())?;
    fs::write(options.work_dir.join("config.capnp"), &config)?;
    Ok(config)
}

fn render_config(
    options: &ConfigOptions,
    modules: &[String],
    asset_manifest: Option<&str>,
) -> RuntimeResult<String> {
    let worker_dir = slash_path(&options.worker_dir)?;

    let mut out = String::new();
    render_root_config(&mut out, options);
    render_assets_router(&mut out, options, asset_manifest);
    render_app_worker(&mut out, options, modules, &worker_dir);

    Ok(out)
}

fn render_root_config(out: &mut String, options: &ConfigOptions) {
    let v8_flags = if options.jitless {
        // V8 only auto-implies --wasm-jitless from --jitless on tvOS; macOS
        // and iOS need it passed explicitly for WASM to run under jitless.
        r#"["--jitless", "--wasm-jitless"]"#
    } else {
        "[]"
    };

    out.push_str(
        r#"@0xdd6a4727576407e9;

using Workerd = import "/workerd/workerd.capnp";

const config :Workerd.Config = (
  services = [
    (name = "main", worker = .appWorker),
"#,
    );

    if options.wrangler_config.assets.is_some() {
        let _ = write!(
            out,
            r#"    (name = "assets-disk", disk = "{PACKAGED_ASSETS_DIR_NAME}"),
    (name = "assets", worker = .assetsRouter),
"#
        );
    }

    let _ = write!(
        out,
        r#"  ],
  sockets = [
    (name = "http", address = "*",
     https = (
       options = (),
       tlsOptions = (
         keypair = (
           privateKey = embed "server.key.pem",
           certificateChain = embed "server.cert.pem",
         ),
         requireClientCerts = true,
         trustedCertificates = [embed "ca.cert.pem"],
       ),
     ),
     service = "main"),
  ],
  v8Flags = {v8_flags},
);

"#
    );
}

fn render_assets_router(out: &mut String, options: &ConfigOptions, asset_manifest: Option<&str>) {
    let Some(manifest) = asset_manifest else {
        return;
    };

    let compatibility_date = escape_capnp_text(&options.wrangler_config.compatibility_date);
    let compatibility_flags =
        render_capnp_string_list(&options.wrangler_config.compatibility_flags);
    let _ = write!(
        out,
        r#"const assetsRouter :Workerd.Worker = (
  compatibilityDate = "{compatibility_date}",
  compatibilityFlags = {compatibility_flags},
  modules = [
    (name = "index", esModule = embed "{ASSETS_WORKER_FILE_NAME}"),
  ],
  bindings = [
    (name = "__ASSET_FILES", service = "assets-disk"),
    (name = "__ASSET_MANIFEST", json = "{manifest}"),
  ],
);

"#,
        manifest = escape_capnp_text(manifest),
    );
}

fn render_app_worker(
    out: &mut String,
    options: &ConfigOptions,
    modules: &[String],
    worker_dir: &str,
) {
    let compatibility_date = escape_capnp_text(&options.wrangler_config.compatibility_date);
    let compatibility_flags =
        render_capnp_string_list(&options.wrangler_config.compatibility_flags);

    let _ = write!(
        out,
        r#"const appWorker :Workerd.Worker = (
  compatibilityDate = "{compatibility_date}",
  compatibilityFlags = {compatibility_flags},
  modules = [
"#
    );

    for module in modules {
        // workerd compiles wasm modules at worker load time, the only point
        // its embedder policy (matching production Workers) permits wasm code
        // generation; imports then receive a ready WebAssembly.Module.
        let field = if is_wasm_module(module) { "wasm" } else { "esModule" };
        let _ = writeln!(
            out,
            "    (name = \"{}\", {field} = embed \"{worker_dir}/{}\"),",
            escape_capnp_text(module),
            module
        );
    }

    out.push_str("  ],\n");

    if let Some(assets) = options.wrangler_config.assets.as_ref() {
        let binding = escape_capnp_text(&assets.binding);
        let _ = write!(
            out,
            r#"  bindings = [
    (name = "{binding}", service = "assets"),
  ],
"#
        );
    }

    out.push_str(");\n");
}

fn is_wasm_module(module: &str) -> bool {
    Path::new(module)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wasm"))
}

fn scan_modules(
    work_dir: &Path,
    worker_dir: &Path,
    worker_main_module: &str,
) -> RuntimeResult<Vec<String>> {
    let abs_dir = work_dir.join(worker_dir);
    let mut modules = Vec::new();
    let main_path = abs_dir.join(worker_main_module);
    if main_path.is_file() {
        modules.push(worker_main_module.to_owned());
    }

    let mut discovered = Vec::new();
    if abs_dir.is_dir() {
        for entry in WalkDir::new(&abs_dir) {
            let entry = entry.map_err(std::io::Error::other)?;
            if !entry.file_type().is_file() {
                continue;
            }

            let rel_path = entry
                .path()
                .strip_prefix(&abs_dir)
                .map_err(|_| RuntimeError::InvalidUtf8Path(entry.path().to_owned()))?;
            let is_module = rel_path.extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("mjs") || extension.eq_ignore_ascii_case("wasm")
            });
            let rel_path = slash_path(rel_path)?;
            if !is_module || rel_path == worker_main_module {
                continue;
            }
            discovered.push(rel_path);
        }
    }

    discovered.sort();
    modules.extend(discovered);
    Ok(modules)
}

fn build_asset_manifest(
    work_dir: &Path,
    assets: Option<&WranglerAssets>,
) -> RuntimeResult<Option<String>> {
    let Some(assets) = assets else {
        return Ok(None);
    };

    let asset_root = work_dir.join(PACKAGED_ASSETS_DIR_NAME);
    let mut entries = BTreeMap::new();
    if asset_root.is_dir() {
        for entry in WalkDir::new(&asset_root) {
            let entry = entry.map_err(std::io::Error::other)?;
            if !entry.file_type().is_file() {
                continue;
            }

            let rel_path = entry
                .path()
                .strip_prefix(&asset_root)
                .map_err(|_| RuntimeError::InvalidUtf8Path(entry.path().to_owned()))?;
            let rel_path = slash_path(rel_path)?;
            let content_type = mime_guess::from_path(entry.path())
                .first_or_octet_stream()
                .essence_str()
                .to_owned();
            entries.insert(rel_path, content_type);
        }
    }

    Ok(Some(
        serde_json::json!({
            "files": entries,
            "htmlHandling": assets.html_handling.as_str(),
            "notFoundHandling": assets.not_found_handling.as_str(),
        })
        .to_string(),
    ))
}

fn render_capnp_string_list(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| format!("\"{}\"", escape_capnp_text(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

fn slash_path(path: &Path) -> RuntimeResult<String> {
    let path = path
        .to_str()
        .ok_or_else(|| RuntimeError::InvalidUtf8Path(path.to_owned()))?;
    Ok(path.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn escape_capnp_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            _ => escaped.push(c),
        }
    }
    escaped
}
