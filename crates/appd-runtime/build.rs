use std::env;
use std::io;
use std::process::Command;

const IOS_MINIMUM_VERSION: &str = "17.0";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=APPD_BARE_SDK_DIR");
    if env::var_os("CARGO_FEATURE_BARE_RUNTIME").is_none()
        || env::var_os("CARGO_FEATURE_BARE_TEST_STUBS").is_some()
    {
        return Ok(());
    }
    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    link_bare()?;
    link_frameworks(&target_os)?;
    if target_os == "ios" {
        link_ios_deployment_target()?;
        link_ios_compiler_runtime()?;
    }
    Ok(())
}

fn link_bare() -> Result<(), io::Error> {
    let encoded = env::var("DEP_APPD_BARE_LINK_ARGS").map_err(io::Error::other)?;
    let arguments: Vec<String> = serde_json::from_str(&encoded).map_err(io::Error::other)?;
    for argument in arguments {
        println!("cargo:rustc-link-arg={argument}");
    }
    Ok(())
}

fn link_frameworks(target_os: &str) -> Result<(), io::Error> {
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
        _ => {
            return Err(io::Error::other(format!(
                "unsupported Bare host: {target_os}"
            )));
        }
    };
    for framework in frameworks {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    Ok(())
}

fn link_ios_deployment_target() -> Result<(), io::Error> {
    let output = Command::new("xcrun")
        .args(["--sdk", "iphoneos", "--show-sdk-version"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("failed to read the iPhoneOS SDK version"));
    }
    let sdk = String::from_utf8_lossy(&output.stdout);
    println!(
        "cargo:rustc-link-arg=-Wl,-platform_version,ios,{IOS_MINIMUM_VERSION},{}",
        sdk.trim()
    );
    Ok(())
}

fn link_ios_compiler_runtime() -> Result<(), io::Error> {
    let output = Command::new("xcrun")
        .args([
            "clang",
            "--target=arm64-apple-ios",
            "-print-file-name=libclang_rt.ios.a",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            "failed to locate the iOS compiler runtime",
        ));
    }
    println!(
        "cargo:rustc-link-arg={}",
        String::from_utf8_lossy(&output.stdout).trim()
    );
    Ok(())
}
