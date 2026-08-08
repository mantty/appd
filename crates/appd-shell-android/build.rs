use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=APPD_BARE_SDK_DIR");
    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    if target_os != "android" {
        return Ok(());
    }
    if env::var_os("CARGO_FEATURE_TEST_STUBS").is_none() {
        link_bare()?;
    }
    println!("cargo:rustc-link-lib=log");
    link_compiler_runtime()?;
    Ok(())
}

fn link_bare() -> Result<(), io::Error> {
    let sdk = env::var_os("APPD_BARE_SDK_DIR").map_or_else(
        || workspace_root().join("target/bare/sdk/android-arm64"),
        PathBuf::from,
    );
    let runtime = sdk.join("runtime");
    if !runtime.join("libbare-kit.so").is_file() {
        return Err(io::Error::other("BareKit runtime is missing"));
    }
    println!("cargo:rustc-link-search=native={}", runtime.display());
    println!("cargo:rustc-link-lib=dylib=bare-kit");
    Ok(())
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn link_compiler_runtime() -> Result<(), io::Error> {
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
