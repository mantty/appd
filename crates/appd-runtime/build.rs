use std::env;
use std::path::PathBuf;

fn main() {
    let Ok(target) = env::var("TARGET") else {
        return;
    };
    if env::var_os("CARGO_FEATURE_NATIVE").is_none() || !target.ends_with("-apple-darwin") {
        return;
    }

    let sdk = env::var_os("APPD_BARE_SDK_DIR").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("target/bare/sdk")
                .join(apple_sdk_target(&target))
        },
        PathBuf::from,
    );
    println!(
        "cargo:rustc-link-arg=-Wl,-rpath,{}",
        sdk.join("runtime").display()
    );
}

fn apple_sdk_target(target: &str) -> &'static str {
    if target.starts_with("aarch64-") {
        "macos-arm64"
    } else {
        "macos-x64"
    }
}
