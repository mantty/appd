#![cfg(all(feature = "native", target_os = "macos"))]

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use appd_bundle::AppLayout;
use appd_bundle::environment::{WorkerEnvironment, write as write_environment};
use appd_runtime::{Config, Runtime};
use rcgen::{CertificateParams, KeyPair};
use serde_json::json;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn starts_a_packaged_worker_with_its_declared_environment() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let root = workspace_root()?;
    let host = "app.appd.local";
    let app = AppLayout::new(temporary.path().join("app"));
    fs::create_dir_all(app.root())?;
    write_environment(
        &app,
        &WorkerEnvironment {
            vars: BTreeMap::from([
                ("TEXT".to_owned(), json!("value")),
                ("JSON".to_owned(), json!({ "enabled": true })),
            ]),
        },
    )?;
    fs::write(app.worker_bundle(), pack_bundle(temporary.path(), &root)?)?;

    let state = temporary.path().join("state");
    let runtime = Runtime::start(
        Config {
            app,
            state_dir: state.clone(),
            host: host.to_owned(),
        },
        |_| {},
    )?;
    let client = (state.join("client.cert.pem"), state.join("client.key.pem"));
    let authority = state.join("ca.cert.pem");
    let foreign = write_foreign_client(temporary.path())?;
    let script = write_client_script(temporary.path())?;

    assert!(
        !connect(&script, runtime.port(), host, &authority, None)?
            .status
            .success()
    );
    assert!(
        connect(&script, runtime.port(), host, &authority, Some(&client))?
            .status
            .success()
    );
    assert!(
        !connect(&script, runtime.port(), host, &authority, Some(&foreign))?
            .status
            .success()
    );
    assert!(
        !connect(&script, runtime.port(), host, &foreign.0, Some(&client))?
            .status
            .success()
    );
    Ok(())
}

fn pack_bundle(directory: &Path, workspace: &Path) -> TestResult<Vec<u8>> {
    let modules = directory.join("node_modules");
    stage_modules(
        &workspace
            .join("target/bare/modules")
            .join(target())
            .join("node_modules"),
        &modules,
    )?;
    let worker = modules.join("appd-worker");
    fs::create_dir_all(&worker)?;
    fs::write(worker.join("package.json"), "{\"type\":\"module\"}")?;
    fs::write(
        worker.join("index.js"),
        r#"import assert from "node:assert";
import http2 from "node:http2";
import process from "node:process";
import { MockTracker } from "node:test";

assert(true);
const mock = new MockTracker().fn();

export default {
  fetch: (_request, env) => {
    const valid = env.TEXT === "value"
      && env.JSON?.enabled === true
      && process.env.TEXT === "value"
      && process.env.JSON === '{"enabled":true}'
      && process.env.PATH === undefined
      && typeof http2 === "function"
      && mock() === undefined;
    return new Response(null, { status: valid ? 204 : 500 });
  }
};
"#,
    )?;
    fs::write(directory.join("package.json"), r#"{"type":"module"}"#)?;
    symlink(
        workspace.join("target/runtime-js"),
        directory.join("runtime"),
    )?;

    let entry = directory.join("appd-worklet.cjs");
    compile_worklet(workspace, directory, &entry)?;
    let bundle = directory.join("worker.bundle");
    let status = Command::new("node")
        .arg(workspace.join("bare/pack/node_modules/bare-pack/bin.js"))
        .arg("--builtins")
        .arg(
            workspace
                .join("target/bare/sdk")
                .join(target())
                .join("builtins.json"),
        )
        .args(["--host", bare_host(), "--base", "/", "--out"])
        .arg(&bundle)
        .arg(entry)
        .current_dir(directory)
        .status()?;
    if !status.success() {
        return Err(format!("bare-pack failed with {status}").into());
    }
    Ok(fs::read(bundle)?)
}

fn compile_worklet(workspace: &Path, directory: &Path, output: &Path) -> TestResult {
    let status = Command::new("node")
        .arg(directory.join("runtime/pack-worker.js"))
        .arg("--compiler")
        .arg(workspace.join("node_modules/esbuild/bin/esbuild"))
        .arg("--worker")
        .arg(directory.join("node_modules/appd-worker"))
        .arg("--output")
        .arg(output)
        .current_dir(directory)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("esbuild failed with {status}").into())
    }
}

fn stage_modules(source: &Path, destination: &Path) -> TestResult {
    if !source.is_dir() {
        return Err(format!("Bare modules are missing: {}", source.display()).into());
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        symlink(
            fs::canonicalize(entry.path())?,
            destination.join(entry.file_name()),
        )?;
    }
    Ok(())
}

fn write_foreign_client(directory: &Path) -> TestResult<(PathBuf, PathBuf)> {
    let key = KeyPair::generate()?;
    let certificate = CertificateParams::default().self_signed(&key)?;
    let cert_path = directory.join("foreign.cert.pem");
    let key_path = directory.join("foreign.key.pem");
    fs::write(&cert_path, certificate.pem())?;
    fs::write(&key_path, key.serialize_pem())?;
    Ok((cert_path, key_path))
}

fn write_client_script(directory: &Path) -> TestResult<PathBuf> {
    let script = directory.join("tls-client.mjs");
    fs::write(&script, include_str!("fixtures/tls-client.mjs"))?;
    Ok(script)
}

fn connect(
    script: &Path,
    port: u16,
    host: &str,
    authority: &Path,
    client: Option<&(PathBuf, PathBuf)>,
) -> TestResult<Output> {
    let mut command = Command::new("node");
    command
        .arg(script)
        .arg(port.to_string())
        .arg(host)
        .arg(authority);
    if let Some((certificate, key)) = client {
        command.arg(certificate).arg(key);
    } else {
        command.args(["", ""]);
    }
    Ok(command.output()?)
}

fn target() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "macos-arm64"
    } else {
        "macos-x64"
    }
}

fn bare_host() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "darwin-arm64"
    } else {
        "darwin-x64"
    }
}

fn workspace_root() -> TestResult<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "appd-runtime is not inside the workspace".into())
}
