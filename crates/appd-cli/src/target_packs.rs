use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_target_pack::{
    Artifact, ArtifactKind, Target, TargetPackManifest, TargetPackVersion, write_manifest,
};

use crate::BuildPlatform;
use crate::build::support::{copy_dir_contents, copy_file};

const TARGET_PACK_DIR_ENV: &str = "appd_target_pack_dir";
const ANDROID_RUNTIME_LIBRARY: &str = "libappd_shell_android.so";
const APPLE_RUNTIME_LIBRARY: &str = "libappd_shell_apple.a";
const WINDOWS_RUNTIME_EXECUTABLE: &str = "appd-shell-windows.exe";

pub(crate) fn resolve_manifest(
    platform: BuildPlatform,
    explicit_manifest: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(manifest) = explicit_manifest {
        return Ok(manifest.to_path_buf());
    }

    let target = default_target(platform)?;
    if let Some(root) = env::var_os(TARGET_PACK_DIR_ENV) {
        return manifest_from_root(Path::new(&root), target).with_context(|| {
            format!("{TARGET_PACK_DIR_ENV} does not contain a target pack for {target}")
        });
    }

    if let Some(manifest) = bundled_manifest(target) {
        return Ok(manifest);
    }

    bail!(
        "no target pack found for {target}; pass --target-pack or build one explicitly with `appd pack build --target {target}`"
    )
}

fn default_target(platform: BuildPlatform) -> Result<Target> {
    match platform {
        BuildPlatform::Android => Ok(Target::AndroidArm64),
        BuildPlatform::Ios => Ok(Target::IosArm64),
        BuildPlatform::IosSimulator if cfg!(target_arch = "aarch64") => {
            Ok(Target::IosSimulatorArm64)
        }
        BuildPlatform::IosSimulator if cfg!(target_arch = "x86_64") => Ok(Target::IosSimulatorX64),
        BuildPlatform::IosSimulator => {
            bail!("iOS Simulator builds require an Intel or Apple Silicon host")
        }
        BuildPlatform::Macos if cfg!(target_arch = "aarch64") => Ok(Target::MacosArm64),
        BuildPlatform::Macos if cfg!(target_arch = "x86_64") => Ok(Target::MacosX64),
        BuildPlatform::Macos => bail!("macOS builds require an Intel or Apple Silicon host"),
        BuildPlatform::Windows if cfg!(all(target_os = "windows", target_arch = "x86_64")) => {
            Ok(Target::WindowsX64)
        }
        BuildPlatform::Windows => bail!("Windows builds require a 64-bit Windows host"),
    }
}

fn manifest_from_root(root: &Path, target: Target) -> Result<PathBuf> {
    let manifest = root.join(target.to_string()).join("target-pack.json");
    if manifest.is_file() {
        Ok(manifest)
    } else {
        bail!("target-pack manifest not found: {}", manifest.display())
    }
}

fn bundled_manifest(target: Target) -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    for root in [
        exe_dir.join("target-packs"),
        exe_dir.join("../share/appd/target-packs"),
        exe_dir.join("../Resources/target-packs"),
    ] {
        let manifest = root.join(target.to_string()).join("target-pack.json");
        if manifest.is_file() {
            return Some(manifest);
        }
    }
    None
}

fn source_workspace_root() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir.parent()?.parent()?.to_path_buf();
    if workspace.join("Cargo.toml").is_file() && workspace.join("crates/appd-runtime").is_dir() {
        Some(workspace)
    } else {
        None
    }
}

pub(crate) fn build_source_target_pack(target: Target) -> Result<PathBuf> {
    let workspace = source_workspace_root()
        .context("source workspace is unavailable; run this command from an appd checkout")?;
    build_source_target_pack_at(&workspace, target)
}

fn build_source_target_pack_at(workspace: &Path, target: Target) -> Result<PathBuf> {
    let rust_target = rust_runtime_target(target);
    let pack_dir = workspace
        .join("target/appd-target-packs")
        .join(target.to_string());
    reset_dir(&pack_dir)?;
    let library = build_runtime_library(workspace, target, rust_target)?;
    let (runtime_kind, runtime_path) = package_runtime(workspace, target, &library, &pack_dir)?;
    if target != Target::WindowsX64 {
        install_native_shell(workspace, target, &pack_dir)?;
    }
    let tools_dir = pack_dir.join("tools");
    deploy_runtime(workspace, &tools_dir.join("runtime"))?;
    let manifest = TargetPackManifest {
        schema_version: TargetPackVersion::CURRENT,
        appd_version: env!("CARGO_PKG_VERSION").to_owned(),
        target,
        artifacts: target_pack_artifacts(&pack_dir, target, runtime_kind, runtime_path)?,
        required_tools: required_tools(target),
    };
    let manifest_path = pack_dir.join("target-pack.json");
    write_manifest(&manifest_path, &manifest)?;
    Ok(manifest_path)
}

fn package_runtime(
    workspace: &Path,
    target: Target,
    library: &Path,
    pack_dir: &Path,
) -> Result<(ArtifactKind, &'static str)> {
    let runtime = match target {
        Target::AndroidArm64 => {
            let path = "bin/libappd_shell_android.so";
            copy_file(library, pack_dir.join(path))?;
            (ArtifactKind::RuntimeLibrary, path)
        }
        Target::WindowsX64 => {
            let path = "bin/appd-shell-windows.exe";
            copy_file(library, pack_dir.join(path))?;
            (ArtifactKind::RuntimeExecutable, path)
        }
        _ => {
            package_apple_framework(workspace, library, pack_dir)?;
            (
                ArtifactKind::RuntimeLibrary,
                "frameworks/AppdRuntime.framework",
            )
        }
    };
    Ok(runtime)
}

fn target_pack_artifacts(
    pack_dir: &Path,
    target: Target,
    runtime_kind: ArtifactKind,
    runtime_path: &str,
) -> Result<Vec<Artifact>> {
    let mut artifacts = vec![packaged_artifact(pack_dir, runtime_kind, runtime_path)?];
    if target != Target::WindowsX64 {
        artifacts.push(packaged_artifact(
            pack_dir,
            ArtifactKind::NativeShellDirectory,
            "native-shell",
        )?);
    }
    artifacts.extend([
        packaged_artifact(
            pack_dir,
            ArtifactKind::RuntimeJavaScriptDirectory,
            "tools/runtime/runtime-js",
        )?,
        packaged_artifact(
            pack_dir,
            ArtifactKind::EsbuildExecutable,
            esbuild_executable(),
        )?,
    ]);
    Ok(artifacts)
}

fn esbuild_executable() -> &'static str {
    "tools/runtime/node_modules/esbuild/bin/esbuild"
}

fn required_tools(target: Target) -> Vec<String> {
    match target {
        Target::AndroidArm64 => vec!["gradle".to_owned()],
        Target::WindowsX64 => Vec::new(),
        _ => vec!["xcrun".to_owned()],
    }
}

fn build_runtime_library(workspace: &Path, target: Target, rust_target: &str) -> Result<PathBuf> {
    eprintln!("Building the appd runtime library for {target}...");
    let mut cargo = Command::new("cargo");
    cargo.args([
        "build",
        "-p",
        shell_package(target),
        "--release",
        "--target",
        rust_target,
    ]);
    if target == Target::WindowsX64 {
        cargo.args(["--bin", "appd-shell-windows"]);
    } else {
        cargo.arg("--lib");
    }
    match target {
        Target::AndroidArm64 => configure_android_toolchain(&mut cargo)?,
        Target::WindowsX64 => {}
        _ => configure_apple_deployment_target(&mut cargo, target),
    }
    let status = cargo
        .current_dir(workspace)
        .status()
        .context("failed to run cargo build for the appd shell")?;
    if !status.success() {
        bail!("appd shell build failed with status {status}");
    }

    let library = workspace
        .join("target")
        .join(rust_target)
        .join("release")
        .join(runtime_artifact_name(target));
    if !library.is_file() {
        bail!("runtime library was not produced: {}", library.display());
    }

    Ok(library)
}

fn configure_apple_deployment_target(command: &mut Command, target: Target) {
    match target {
        Target::MacosArm64 | Target::MacosX64 => {
            command.env("MACOSX_DEPLOYMENT_TARGET", "14.0");
        }
        Target::IosArm64 => {
            command.env("IPHONEOS_DEPLOYMENT_TARGET", "17.0");
        }
        Target::IosSimulatorArm64 | Target::IosSimulatorX64 => {
            command
                .env("IPHONEOS_DEPLOYMENT_TARGET", "17.0")
                .env("IPHONESIMULATOR_DEPLOYMENT_TARGET", "17.0");
        }
        Target::AndroidArm64 | Target::WindowsX64 => {
            unreachable!("target has its own toolchain configuration")
        }
    }
}

fn install_native_shell(workspace: &Path, target: Target, pack_dir: &Path) -> Result<()> {
    let destination = pack_dir.join("native-shell");
    if target == Target::AndroidArm64 {
        return copy_dir_contents(
            &workspace.join("crates/appd-shell-android/kotlin"),
            &destination,
        );
    }
    if target == Target::WindowsX64 {
        return Ok(());
    }
    for file in ["AppdShell.swift", "AppdPlugins.swift"] {
        copy_file(
            workspace.join("crates/appd-shell-apple/native").join(file),
            destination.join(file),
        )?;
    }
    Ok(())
}

fn package_apple_framework(workspace: &Path, rust_library: &Path, pack_dir: &Path) -> Result<()> {
    let framework = pack_dir.join("frameworks/AppdRuntime.framework");
    fs::create_dir_all(framework.join("Headers"))?;
    fs::create_dir_all(framework.join("Modules"))?;
    copy_file(rust_library, framework.join("AppdRuntime"))?;
    copy_file(
        workspace.join("crates/appd-shell-apple/native/AppdRuntime.h"),
        framework.join("Headers/AppdRuntime.h"),
    )?;
    copy_file(
        workspace.join("crates/appd-shell-apple/native/module.modulemap"),
        framework.join("Modules/module.modulemap"),
    )
}

fn configure_android_toolchain(command: &mut Command) -> Result<()> {
    let ndk = android_ndk_dir()?;
    let prebuilt = ndk.join("toolchains/llvm/prebuilt");
    let mut hosts = fs::read_dir(&prebuilt)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir());
    let Some(host) = hosts.next() else {
        bail!("Android NDK toolchain is missing: {}", prebuilt.display());
    };
    let compiler = host.join("bin/aarch64-linux-android31-clang");
    if !compiler.is_file() {
        bail!("Android NDK compiler is missing: {}", compiler.display());
    }
    let sysroot = host.join("sysroot");
    command.env("CC_aarch64_linux_android", &compiler);
    command.env("CC_aarch64_unknown_linux_android", &compiler);
    let ar = host.join("bin/llvm-ar");
    let ranlib = host.join("bin/llvm-ranlib");
    command.env("AR_aarch64_linux_android", &ar);
    command.env("AR_aarch64_unknown_linux_android", &ar);
    command.env("RANLIB_aarch64_linux_android", &ranlib);
    command.env("RANLIB_aarch64_unknown_linux_android", &ranlib);
    command.env("RANLIB", &ranlib);
    command.env(
        "BINDGEN_EXTRA_CLANG_ARGS",
        format!(
            "--target=aarch64-linux-android31 --sysroot={}",
            sysroot.display()
        ),
    );
    command.env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", compiler);
    Ok(())
}

fn android_ndk_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("ANDROID_NDK_HOME").map(PathBuf::from)
        && path.is_dir()
    {
        return Ok(path);
    }
    let Some(root) = env::var_os("ANDROID_HOME")
        .or_else(|| env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
    else {
        bail!("set ANDROID_NDK_HOME or ANDROID_HOME to build Android target packs");
    };
    let ndks = root.join("ndk");
    let mut versions = fs::read_dir(&ndks)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    versions.sort();
    versions
        .pop()
        .with_context(|| format!("Android NDK is missing: {}", ndks.display()))
}

fn packaged_artifact(root: &Path, kind: ArtifactKind, path: &str) -> Result<Artifact> {
    let artifact = root.join(path);
    if !artifact.exists() {
        bail!(
            "target-pack artifact was not produced: {}",
            artifact.display()
        );
    }
    Ok(Artifact {
        kind,
        path: path.to_owned(),
    })
}

fn deploy_runtime(workspace: &Path, output: &Path) -> Result<()> {
    reset_dir(output)?;
    copy_dir_contents(
        &workspace.join("node_modules/esbuild"),
        &output.join("node_modules/esbuild"),
    )?;
    copy_dir_contents(&workspace.join("runtime/qjs"), &output.join("runtime-js"))
}

fn shell_package(target: Target) -> &'static str {
    match target {
        Target::AndroidArm64 => "appd-shell-android",
        Target::WindowsX64 => "appd-shell-windows",
        _ => "appd-shell-apple",
    }
}

fn rust_runtime_target(target: Target) -> &'static str {
    match target {
        Target::AndroidArm64 => "aarch64-linux-android",
        Target::MacosArm64 => "aarch64-apple-darwin",
        Target::MacosX64 => "x86_64-apple-darwin",
        Target::IosArm64 => "aarch64-apple-ios",
        Target::IosSimulatorArm64 => "aarch64-apple-ios-sim",
        Target::IosSimulatorX64 => "x86_64-apple-ios",
        Target::WindowsX64 => "x86_64-pc-windows-msvc",
    }
}

fn runtime_artifact_name(target: Target) -> &'static str {
    match target {
        Target::AndroidArm64 => ANDROID_RUNTIME_LIBRARY,
        Target::WindowsX64 => WINDOWS_RUNTIME_EXECUTABLE,
        _ => APPLE_RUNTIME_LIBRARY,
    }
}

fn reset_dir(path: &Path) -> Result<()> {
    clear_path(path)?;
    fs::create_dir_all(path)?;
    Ok(())
}

fn clear_path(path: &Path) -> Result<()> {
    if path.is_symlink() || path.is_file() {
        fs::remove_file(path)?;
    } else if path.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{esbuild_executable, packaged_artifact};
    use appd_target_pack::ArtifactKind;

    #[test]
    fn uses_the_packaged_esbuild_script() {
        assert_eq!(
            esbuild_executable(),
            "tools/runtime/node_modules/esbuild/bin/esbuild"
        );
    }

    #[test]
    fn rejects_missing_pack_artifacts() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let result = packaged_artifact(
            directory.path(),
            ArtifactKind::RuntimeJavaScriptDirectory,
            "runtime",
        );

        assert!(result.is_err_and(|error| {
            error
                .to_string()
                .contains("target-pack artifact was not produced")
        }));
        Ok(())
    }
}
