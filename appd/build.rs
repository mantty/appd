use std::env;
use std::io;
use std::path::Path;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    if target_os != "android" {
        return Ok(());
    }
    println!("cargo:rustc-link-lib=log");
    link_compiler_runtime()?;
    Ok(())
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
