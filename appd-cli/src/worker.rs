//! Worker package preparation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd::compile_module;
use appd::{
    PackageLayout, WorkerEnvironment, WorkerManifest, WranglerConfig, WranglerModuleType,
    WranglerRule, compress_worker_module, write_asset_manifest, write_worker_environment,
    write_worker_manifest,
};
use appd_cli::{ArtifactKind, TargetPackManifest};
use serde::Deserialize;
use walkdir::WalkDir;

use super::support::{artifact_path, copy_dir_contents, copy_file};

const WORKER_ENTRY: &str = "entry.js";

pub(crate) fn prepare_quickjs_app(
    app_dir: &Path,
    pack_root: &Path,
    manifest: &TargetPackManifest,
    wrangler: &WranglerConfig,
) -> Result<()> {
    let layout = PackageLayout::new(app_dir);
    write_worker_environment(
        &layout,
        &WorkerEnvironment {
            vars: wrangler.vars.clone(),
        },
    )?;
    fs::create_dir_all(layout.bundle())?;
    if let Some(assets) = &wrangler.assets {
        copy_dir_contents(&assets.directory, &layout.assets())?;
        write_asset_manifest(&layout, assets)?;
    }
    compile_worker_bundle(&layout, wrangler, pack_root, manifest)
}

fn compile_worker_bundle(
    layout: &PackageLayout,
    wrangler: &WranglerConfig,
    pack_root: &Path,
    manifest: &TargetPackManifest,
) -> Result<()> {
    let (source, metafile) = run_esbuild(layout, wrangler, pack_root, manifest)?;
    write_worker_modules(layout, &source)?;
    let inputs = read_metafile_inputs(&metafile)?;
    package_bundle_files(wrangler, layout, &inputs)?;
    fs::remove_dir_all(source)?;
    if metafile.exists() {
        fs::remove_file(metafile)?;
    }
    Ok(())
}

fn run_esbuild(
    layout: &PackageLayout,
    wrangler: &WranglerConfig,
    pack_root: &Path,
    manifest: &TargetPackManifest,
) -> Result<(PathBuf, PathBuf)> {
    let runtime = artifact_path(
        pack_root,
        manifest,
        &ArtifactKind::RuntimeJavaScriptDirectory,
    )?;
    let events = runtime_source(&runtime, "events/events.mjs", "events.mjs");
    let stream = runtime_source(&runtime, "streams/node.mjs", "stream.mjs");
    let builtins = runtime_source(
        &runtime,
        "builtins/cloudflare-workers.mjs",
        "cloudflare-workers.mjs",
    );
    let compiler = artifact_path(pack_root, manifest, &ArtifactKind::EsbuildExecutable)?;
    let source = layout.root().join("worker.source");
    let metafile = layout.root().join("worker.metafile.json");
    if source.exists() {
        fs::remove_dir_all(&source)?;
    }
    fs::create_dir_all(&source)?;

    let mut command = Command::new("node");
    command
        .arg(node_path(&compiler))
        .args([
            "--bundle",
            "--splitting",
            "--format=esm",
            "--platform=neutral",
            "--target=es2022",
            "--entry-names=entry",
            "--chunk-names=chunks/[name]-[hash]",
        ])
        .arg(format!("--outdir={}", node_path(&source).display()))
        .arg(format!("--metafile={}", node_path(&metafile).display()))
        .arg(format!(
            "--alias:node:events={}",
            node_path(&events).display()
        ))
        .arg(format!(
            "--alias:node:stream={}",
            node_path(&stream).display()
        ))
        .arg("--external:node:fs")
        .arg("--external:node:fs/promises")
        .arg(format!(
            "--alias:cloudflare:workers={}",
            node_path(&builtins).display()
        ));
    for (extension, loader) in esbuild_loaders(&wrangler.rules) {
        command.arg(format!("--loader:.{extension}={loader}"));
    }
    let status = command
        .arg(node_path(&wrangler.main))
        .status()
        .context("failed to bundle the Worker with esbuild")?;
    if !status.success() {
        bail!("Worker bundling failed with status {status}");
    }
    Ok((source, metafile))
}

fn runtime_source(runtime: &Path, current: &str, legacy: &str) -> PathBuf {
    let current = runtime.join(current);
    if current.is_file() {
        current
    } else {
        runtime.join(legacy)
    }
}

fn write_worker_modules(layout: &PackageLayout, source: &Path) -> Result<()> {
    let source_files = javascript_outputs(source)?;
    let entry = source.join(WORKER_ENTRY);
    if !entry.is_file() {
        bail!(
            "esbuild did not emit the Worker entry module {}",
            entry.display()
        );
    }
    let modules = layout.worker_modules();
    fs::create_dir_all(&modules)?;
    for source_file in source_files {
        let relative = source_file
            .strip_prefix(source)
            .context("Worker module escaped esbuild output directory")?;
        let name = slash_path(relative)?;
        let bytecode = compile_module(&name, &fs::read(&source_file)?)?;
        let destination = modules.join(format!("{name}.qjs"));
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, compress_worker_module(&bytecode)?)?;
    }
    write_worker_manifest(
        layout,
        &WorkerManifest {
            entry: WORKER_ENTRY.to_owned(),
        },
    )?;
    Ok(())
}

fn javascript_outputs(source: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(source) {
        let entry = entry.map_err(std::io::Error::other)?;
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| matches!(extension.to_str(), Some("js" | "mjs" | "cjs")))
        {
            paths.push(entry.path().to_owned());
        }
    }
    paths.sort();
    Ok(paths)
}

#[derive(Deserialize)]
struct EsbuildMetafile {
    #[serde(default)]
    inputs: BTreeMap<String, serde_json::Value>,
}

fn read_metafile_inputs(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let metafile: EsbuildMetafile = serde_json::from_slice(&fs::read(path)?)?;
    Ok(metafile.inputs.into_keys().map(PathBuf::from).collect())
}

fn package_bundle_files(
    wrangler: &WranglerConfig,
    layout: &PackageLayout,
    inputs: &[PathBuf],
) -> Result<()> {
    let mut files = BTreeSet::new();
    if wrangler.find_additional_modules {
        for entry in WalkDir::new(&wrangler.base_dir).follow_links(true) {
            let entry = entry.map_err(std::io::Error::other)?;
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry.path().strip_prefix(&wrangler.base_dir)?;
            if rule_type(relative, &wrangler.rules).is_some_and(WranglerModuleType::is_bundle_file)
            {
                files.insert(entry.path().to_owned());
            }
        }
    }
    for input in inputs {
        if is_code_path(input) {
            continue;
        }
        let path = if input.is_absolute() {
            input.clone()
        } else {
            std::env::current_dir()?.join(input)
        };
        if path.is_file() && relative_to_base(&path, &wrangler.base_dir).is_some() {
            files.insert(path);
        }
    }

    for file in files {
        let relative = relative_to_base(&file, &wrangler.base_dir)
            .context("bundle file is outside Wrangler base_dir")?;
        copy_file(&file, layout.bundle().join(relative))?;
    }
    Ok(())
}

fn rule_type(path: &Path, rules: &[WranglerRule]) -> Option<WranglerModuleType> {
    let path = slash_path(path).ok()?;
    let mut selected = None;
    for rule in rules {
        if rule.globs.iter().any(|glob| glob_matches(glob, &path)) {
            selected = Some(rule.module_type);
            if !rule.fallthrough {
                break;
            }
        }
    }
    selected
}

fn relative_to_base(path: &Path, base: &Path) -> Option<PathBuf> {
    let path = fs::canonicalize(path).ok()?;
    let base = fs::canonicalize(base).ok()?;
    let relative = path.strip_prefix(base).ok()?;
    (!relative.as_os_str().is_empty()).then(|| relative.to_owned())
}

fn is_code_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "mts" | "cts"
            )
        })
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim_start_matches("./");
    let path = path.trim_start_matches("./");
    let pattern = pattern.split('/').collect::<Vec<_>>();
    let path = path.split('/').collect::<Vec<_>>();
    glob_segments(&pattern, &path)
}

fn glob_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern {
        [] => path.is_empty(),
        ["**", rest @ ..] => {
            glob_segments(rest, path) || (!path.is_empty() && glob_segments(pattern, &path[1..]))
        }
        [segment, rest @ ..] => {
            !path.is_empty() && segment_matches(segment, path[0]) && glob_segments(rest, &path[1..])
        }
    }
}

fn segment_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut states = vec![(0, 0)];
    while let Some((pattern_index, value_index)) = states.pop() {
        if pattern_index == pattern.len() {
            if value_index == value.len() {
                return true;
            }
            continue;
        }
        match pattern[pattern_index] {
            b'*' => {
                states.push((pattern_index + 1, value_index));
                if value_index < value.len() {
                    states.push((pattern_index, value_index + 1));
                }
            }
            b'?' if value_index < value.len() => {
                states.push((pattern_index + 1, value_index + 1));
            }
            character if value_index < value.len() && character == value[value_index] => {
                states.push((pattern_index + 1, value_index + 1));
            }
            _ => {}
        }
    }
    false
}

fn esbuild_loaders(rules: &[WranglerRule]) -> BTreeMap<String, &'static str> {
    let mut loaders = BTreeMap::from([
        ("txt".to_owned(), "text"),
        ("html".to_owned(), "text"),
        ("sql".to_owned(), "text"),
        ("bin".to_owned(), "binary"),
        ("wasm".to_owned(), "binary"),
    ]);
    for rule in rules {
        let loader = match rule.module_type {
            WranglerModuleType::Text => "text",
            WranglerModuleType::Data | WranglerModuleType::CompiledWasm => "binary",
            WranglerModuleType::EsModule | WranglerModuleType::CommonJs => continue,
        };
        for glob in &rule.globs {
            let Some(extension) = glob
                .rsplit('/')
                .next()
                .and_then(|name| name.strip_prefix("*."))
            else {
                continue;
            };
            loaders.insert(extension.to_owned(), loader);
        }
    }
    loaders
}

fn slash_path(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Worker path is not valid UTF-8: {}", path.display()))?;
    Ok(path.replace(std::path::MAIN_SEPARATOR, "/"))
}

fn node_path(path: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(value) = value.strip_prefix(r"\\?\")
            && value.as_bytes().get(1) == Some(&b':')
        {
            return value.into();
        }
    }
    path.into()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::runtime_source;

    #[test]
    fn selects_the_nested_runtime_source() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let nested = directory.path().join("events/events.mjs");
        fs::create_dir_all(nested.parent().ok_or("runtime source has no parent")?)?;
        fs::write(&nested, "export {};")?;

        assert_eq!(
            runtime_source(directory.path(), "events/events.mjs", "events.mjs"),
            nested
        );
        Ok(())
    }

    #[test]
    fn falls_back_to_the_flat_runtime_source() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let legacy = directory.path().join("events.mjs");
        fs::write(&legacy, "export {};")?;

        assert_eq!(
            runtime_source(directory.path(), "events/events.mjs", "events.mjs"),
            legacy
        );
        Ok(())
    }
}
