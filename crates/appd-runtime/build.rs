use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct WorkerdSdkManifest {
    link_args: Vec<String>,
    link_inputs: Vec<WorkerdLinkInput>,
}

#[derive(Debug, Deserialize)]
struct WorkerdLinkInput {
    path: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=appd_workerd_out_dir");

    let workerd_ffi = env::var_os("CARGO_FEATURE_WORKERD_FFI").is_some();
    let test_stubs = env::var_os("CARGO_FEATURE_WORKERD_TEST_STUBS").is_some();
    if !workerd_ffi || test_stubs {
        return Ok(());
    }

    let target = env::var("TARGET")?;
    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    let workerd_dir = workerd_sdk_dir(&target)?;
    let manifest_path = workerd_dir.join("sdk-manifest.json");
    if !manifest_path.is_file() {
        return Err(error(format!(
            "workerd SDK manifest not found: {}",
            manifest_path.display()
        ))
        .into());
    }

    link_workerd_sdk(&workerd_dir, &manifest_path)?;
    link_platform_libraries(&target_os)?;
    // aarch64-apple-ios-sim carries the suffix, but x86_64-apple-ios is ALSO
    // the simulator (there was never a physical x86_64 iOS device) -- check
    // the actual ABI cfg rather than pattern-matching the triple.
    let is_simulator = env::var("CARGO_CFG_TARGET_ABI").as_deref() == Ok("sim");
    if target_os == "ios" {
        // rustc's own per-target default (14.0 for the simulator, 10.0 for
        // device) doesn't match the MinimumOSVersion this crate's app bundles
        // declare; App Store validation cross-checks the two.
        link_ios_deployment_target(is_simulator)?;
        if !is_simulator {
            link_ios_device_compiler_rt()?;
        }
    }

    Ok(())
}

const IOS_MINIMUM_OS_VERSION: &str = "17.0";

fn link_ios_deployment_target(is_simulator: bool) -> Result<(), io::Error> {
    let sdk = if is_simulator { "iphonesimulator" } else { "iphoneos" };
    let output = Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-version"])
        .output()
        .map_err(|err| error(format!("xcrun --sdk {sdk} --show-sdk-version failed: {err}")))?;
    if !output.status.success() {
        return Err(error(format!(
            "xcrun --sdk {sdk} --show-sdk-version failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let sdk_version = String::from_utf8_lossy(&output.stdout).trim().to_owned();

    let platform = if is_simulator { "ios-simulator" } else { "ios" };
    println!(
        "cargo:rustc-link-arg=-Wl,-platform_version,{platform},{IOS_MINIMUM_OS_VERSION},{sdk_version}"
    );
    Ok(())
}

fn workerd_sdk_dir(target: &str) -> Result<PathBuf, io::Error> {
    if let Some(path) = env::var_os("appd_workerd_out_dir") {
        return Ok(PathBuf::from(path));
    }

    Ok(workspace_root()?.join("target/workerd/sdk").join(target))
}

fn workspace_root() -> Result<PathBuf, io::Error> {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|err| error(err.to_string()))?);
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| error("appd-runtime crate is not inside the appd workspace"))
}

fn link_workerd_sdk(sdk_dir: &Path, manifest_path: &Path) -> Result<(), io::Error> {
    let manifest: WorkerdSdkManifest = serde_json::from_str(
        &std::fs::read_to_string(manifest_path)
            .map_err(|err| error(format!("failed to read {}: {err}", manifest_path.display())))?,
    )
    .map_err(|err| {
        error(format!(
            "failed to parse {}: {err}",
            manifest_path.display()
        ))
    })?;

    println!("cargo:rerun-if-changed={}", manifest_path.display());
    for input in &manifest.link_inputs {
        let path = sdk_dir.join(&input.path);
        if !path.is_file() {
            return Err(error(format!(
                "workerd SDK link input is missing: {}",
                path.display()
            )));
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }

    for arg in manifest.link_args {
        println!(
            "cargo:rustc-link-arg={}",
            resolve_manifest_link_arg(sdk_dir, &manifest.link_inputs, &arg)
        );
    }

    Ok(())
}

fn resolve_manifest_link_arg(
    sdk_dir: &Path,
    link_inputs: &[WorkerdLinkInput],
    arg: &str,
) -> String {
    let mut resolved = arg.to_owned();
    for input in link_inputs {
        resolved = resolved.replace(
            &input.path,
            &sdk_dir.join(&input.path).display().to_string(),
        );
    }
    resolved
}

fn link_platform_libraries(target_os: &str) -> Result<(), io::Error> {
    println!("cargo:rustc-link-lib=c++");
    println!("cargo:rustc-link-arg=-Wl,-dead_strip");

    let frameworks = match target_os {
        "macos" => [
            "AppKit",
            "WebKit",
            "Security",
            "CoreFoundation",
            "Foundation",
        ],
        "ios" => [
            "UIKit",
            "WebKit",
            "Security",
            "CoreFoundation",
            "Foundation",
        ],
        _ => Err(error(format!(
            "workerd platform libraries are not configured for {target_os}"
        )))?,
    };

    for framework in frameworks {
        println!("cargo:rustc-link-lib=framework={framework}");
    }

    Ok(())
}

fn link_ios_device_compiler_rt() -> Result<(), io::Error> {
    // clang auto-links compiler-rt when it drives the final link; rustc
    // doesn't, but workerd's C++ (compiled for real iOS device, unlike the
    // simulator) still emits calls into its stack-probe support.
    let output = Command::new("xcrun")
        .args([
            "clang",
            "--target=arm64-apple-ios",
            "-print-file-name=libclang_rt.ios.a",
        ])
        .output()
        .map_err(|err| error(format!("xcrun clang -print-file-name failed: {err}")))?;
    if !output.status.success() {
        return Err(error(format!(
            "xcrun clang -print-file-name failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    println!("cargo:rustc-link-arg={path}");
    Ok(())
}

fn error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}
