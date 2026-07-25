use std::env;
use std::io;
use std::path::Path;
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
    if target_os == "macos" {
        println!("cargo:rustc-link-arg=-Wl,-export_dynamic");
    }
    link_platform_libraries(&target_os)?;
    if target_os == "ios" {
        let target = env::var("TARGET")?;
        link_ios_deployment_target(&target)?;
        link_ios_compiler_runtime(&target)?;
    }
    if target_os == "android" {
        link_android_compiler_runtime()?;
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

fn link_platform_libraries(target_os: &str) -> Result<(), io::Error> {
    let frameworks = match target_os {
        "macos" => [
            "AppKit",
            "WebKit",
            "Security",
            "CoreFoundation",
            "Foundation",
            "Network",
        ],
        "ios" => [
            "UIKit",
            "WebKit",
            "Security",
            "CoreFoundation",
            "Foundation",
            "Network",
        ],
        "android" => {
            println!("cargo:rustc-link-lib=log");
            return Ok(());
        }
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

fn link_ios_deployment_target(target: &str) -> Result<(), io::Error> {
    let (sdk, platform) = ios_link_settings(target)?;
    let output = Command::new("xcrun")
        .args(["--sdk", sdk, "--show-sdk-version"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("failed to read the iPhoneOS SDK version"));
    }
    let sdk = String::from_utf8_lossy(&output.stdout);
    println!(
        "cargo:rustc-link-arg=-Wl,-platform_version,{platform},{IOS_MINIMUM_VERSION},{}",
        sdk.trim()
    );
    Ok(())
}

fn link_ios_compiler_runtime(target: &str) -> Result<(), io::Error> {
    let (sdk, _) = ios_link_settings(target)?;
    let (compiler_target, runtime) = match target {
        "aarch64-apple-ios" => ("arm64-apple-ios", "libclang_rt.ios.a"),
        "aarch64-apple-ios-sim" => ("arm64-apple-ios-simulator", "libclang_rt.iossim.a"),
        "x86_64-apple-ios" => ("x86_64-apple-ios-simulator", "libclang_rt.iossim.a"),
        _ => {
            return Err(io::Error::other(format!(
                "unsupported iOS target: {target}"
            )));
        }
    };
    let compiler_target = format!("--target={compiler_target}");
    let runtime = format!("-print-file-name={runtime}");
    let output = Command::new("xcrun")
        .args(["--sdk", sdk, "clang", &compiler_target, &runtime])
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

fn link_android_compiler_runtime() -> Result<(), io::Error> {
    let compiler = env::var("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER")
        .or_else(|_| env::var("CC_aarch64_linux_android"))
        .map_err(io::Error::other)?;
    let output = Command::new(compiler)
        .arg("-print-file-name=libclang_rt.builtins-aarch64-android.a")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            "failed to locate the Android compiler runtime",
        ));
    }
    let runtime = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !Path::new(&runtime).is_file() {
        return Err(io::Error::other("Android compiler runtime is missing"));
    }
    println!("cargo:rustc-link-arg={runtime}");
    Ok(())
}

fn ios_link_settings(target: &str) -> Result<(&'static str, &'static str), io::Error> {
    match target {
        "aarch64-apple-ios" => Ok(("iphoneos", "ios")),
        "aarch64-apple-ios-sim" | "x86_64-apple-ios" => Ok(("iphonesimulator", "ios-simulator")),
        _ => Err(io::Error::other(format!(
            "unsupported iOS target: {target}"
        ))),
    }
}
