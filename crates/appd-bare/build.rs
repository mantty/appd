use std::env;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Deserialize)]
struct SdkManifest {
    schema_version: u32,
    target: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=APPD_BARE_SDK_DIR");
    if env::var_os("CARGO_FEATURE_NATIVE").is_none()
        || env::var_os("CARGO_FEATURE_TEST_STUBS").is_some()
    {
        return Ok(());
    }

    let expected_target = sdk_target(&env::var("TARGET").map_err(io::Error::other)?)?;
    let sdk = sdk_dir(expected_target)?;
    let manifest_path = sdk.join("sdk-manifest.json");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let manifest: SdkManifest = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    if manifest.schema_version != 4 || manifest.target != expected_target {
        return Err(io::Error::other("Bare SDK does not match the Rust target").into());
    }
    link_runtime(&sdk.join("runtime"), expected_target);
    Ok(())
}

fn link_runtime(runtime: &Path, target: &str) {
    println!("cargo:rerun-if-changed={}", runtime.display());
    match target {
        "macos-arm64" | "macos-x64" | "ios-arm64" | "ios-simulator-arm64" | "ios-simulator-x64" => {
            println!("cargo:rustc-link-search=framework={}", runtime.display());
            println!("cargo:rustc-link-lib=framework=BareKit");
            if target.starts_with("macos") {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", runtime.display());
            }
        }
        "android-arm64" | "windows-x64" => {
            println!("cargo:rustc-link-search=native={}", runtime.display());
            println!("cargo:rustc-link-lib=dylib=bare-kit");
        }
        _ => unreachable!("validated Bare target must have a runtime linker rule"),
    }
}

fn sdk_dir(target: &str) -> Result<PathBuf, io::Error> {
    if let Some(path) = env::var_os("APPD_BARE_SDK_DIR") {
        return Ok(PathBuf::from(path));
    }
    Ok(workspace_root()?.join("target/bare/sdk").join(target))
}

fn sdk_target(target: &str) -> Result<&'static str, io::Error> {
    match target {
        "aarch64-apple-darwin" => Ok("macos-arm64"),
        "x86_64-apple-darwin" => Ok("macos-x64"),
        "aarch64-apple-ios" => Ok("ios-arm64"),
        "aarch64-apple-ios-sim" => Ok("ios-simulator-arm64"),
        "x86_64-apple-ios" => Ok("ios-simulator-x64"),
        "aarch64-linux-android" => Ok("android-arm64"),
        "x86_64-pc-windows-msvc" => Ok("windows-x64"),
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
