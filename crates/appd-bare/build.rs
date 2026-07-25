use std::env;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct SdkManifest {
    schema_version: u32,
    target: String,
    module_lock_sha256: String,
    link_args: Vec<String>,
    link_inputs: Vec<LinkInput>,
}

#[derive(Deserialize)]
struct LinkInput {
    path: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=APPD_BARE_SDK_DIR");
    if env::var_os("CARGO_FEATURE_NATIVE").is_none()
        || env::var_os("CARGO_FEATURE_TEST_STUBS").is_some()
    {
        return Ok(());
    }

    let rust_target = env::var("TARGET").map_err(io::Error::other)?;
    let expected_target = sdk_target(&rust_target)?;
    let sdk = sdk_dir(expected_target)?;
    let manifest_path = sdk.join("sdk-manifest.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let manifest: SdkManifest = serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)?;
    validate_manifest(&manifest, expected_target)?;
    validate_module_lock(&manifest)?;
    validate_inputs(&sdk, &manifest)?;
    let arguments = manifest
        .link_args
        .iter()
        .map(|argument| resolve_argument(&sdk, &manifest.link_inputs, argument))
        .collect::<Vec<_>>();
    println!(
        "cargo::metadata=link_args={}",
        serde_json::to_string(&arguments)?
    );
    Ok(())
}

fn sdk_dir(target: &str) -> Result<PathBuf, io::Error> {
    if let Some(path) = env::var_os("APPD_BARE_SDK_DIR") {
        return Ok(PathBuf::from(path));
    }
    Ok(workspace_root()?.join("target/bare/sdk").join(target))
}

fn validate_manifest(manifest: &SdkManifest, target: &str) -> Result<(), io::Error> {
    if manifest.schema_version != 1 {
        return Err(io::Error::other("unsupported Bare SDK schema version"));
    }
    if manifest.target != target {
        return Err(io::Error::other(format!(
            "Bare SDK target {} does not match {target}",
            manifest.target
        )));
    }
    Ok(())
}

fn validate_module_lock(manifest: &SdkManifest) -> Result<(), io::Error> {
    let lock = std::fs::read(workspace_root()?.join("pnpm-lock.yaml"))?;
    let mut actual = String::with_capacity(64);
    for byte in Sha256::digest(lock) {
        write!(actual, "{byte:02x}").map_err(io::Error::other)?;
    }
    if manifest.module_lock_sha256 == actual {
        Ok(())
    } else {
        Err(io::Error::other(
            "Bare SDK native addons do not match pnpm-lock.yaml",
        ))
    }
}

fn sdk_target(target: &str) -> Result<&'static str, io::Error> {
    match target {
        "aarch64-apple-darwin" => Ok("macos-arm64"),
        "x86_64-apple-darwin" => Ok("macos-x64"),
        "aarch64-apple-ios" => Ok("ios-arm64"),
        "aarch64-apple-ios-sim" => Ok("ios-simulator-arm64"),
        "x86_64-apple-ios" => Ok("ios-simulator-x64"),
        "aarch64-linux-android" => Ok("android-arm64"),
        _ => Err(io::Error::other(format!(
            "Bare SDK does not support {target}"
        ))),
    }
}

fn workspace_root() -> Result<PathBuf, io::Error> {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(io::Error::other)?);
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("appd-bare is not inside the workspace"))
}

fn validate_inputs(sdk: &Path, manifest: &SdkManifest) -> Result<(), io::Error> {
    for input in &manifest.link_inputs {
        let path = sdk.join(&input.path);
        if !path.is_file() {
            return Err(io::Error::other(format!(
                "Bare SDK input is missing: {}",
                path.display()
            )));
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
    Ok(())
}

fn resolve_argument(sdk: &Path, inputs: &[LinkInput], argument: &str) -> String {
    inputs.iter().fold(argument.to_owned(), |resolved, input| {
        resolved.replace(&input.path, &sdk.join(&input.path).display().to_string())
    })
}
