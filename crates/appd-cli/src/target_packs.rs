use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_target_pack::{
    Artifact, ArtifactKind, Target, TargetPackManifest, TargetPackVersion, artifact_sha256,
    write_manifest,
};

use crate::BuildPlatform;
use crate::build::support::{copy_dir_contents, copy_file};

const TARGET_PACK_DIR_ENV: &str = "appd_target_pack_dir";
const ANDROID_RUNTIME_LIBRARY: &str = "libappd_shell_android.so";
const APPLE_RUNTIME_LIBRARY: &str = "libappd_shell_apple.a";

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
    build_bare_sdk(workspace, target)?;
    build_runtime_tools(workspace)?;
    let library = build_runtime_library(workspace, target, rust_target)?;

    let runtime_path = if target == Target::AndroidArm64 {
        let path = "bin/libappd_shell_android.so";
        copy_file(&library, pack_dir.join(path))?;
        path
    } else {
        package_apple_framework(workspace, target, &library, &pack_dir)?;
        "frameworks/AppdRuntime.framework"
    };
    if target == Target::AndroidArm64 {
        copy_file(
            &android_bare_host_library(workspace)?,
            pack_dir.join("bin/libappd_bare.so"),
        )?;
    }
    install_native_shell(workspace, target, &pack_dir)?;
    let tools_dir = pack_dir.join("tools");
    deploy_runtime(workspace, &tools_dir.join("runtime"))?;
    install_bare_tls(workspace, &tools_dir.join("runtime"), target)?;
    deploy_packer(workspace, &tools_dir.join("packer"))?;

    let mut artifacts = vec![
        packaged_artifact(&pack_dir, ArtifactKind::RuntimeLibrary, runtime_path)?,
        packaged_artifact(
            &pack_dir,
            ArtifactKind::NativeShellDirectory,
            "native-shell",
        )?,
        packaged_artifact(
            &pack_dir,
            ArtifactKind::RuntimeJavaScriptDirectory,
            "tools/runtime/runtime-js",
        )?,
        packaged_artifact(
            &pack_dir,
            ArtifactKind::BarePackExecutable,
            "tools/packer/node_modules/bare-pack/bin.js",
        )?,
        packaged_artifact(
            &pack_dir,
            ArtifactKind::EsbuildExecutable,
            "tools/packer/node_modules/esbuild/bin/esbuild",
        )?,
    ];
    if target == Target::AndroidArm64 {
        artifacts.push(packaged_artifact(
            &pack_dir,
            ArtifactKind::BareHostLibrary,
            "bin/libappd_bare.so",
        )?);
    }
    let required_tools = if target == Target::AndroidArm64 {
        vec!["node".to_owned(), "gradle".to_owned()]
    } else {
        vec!["node".to_owned(), "xcrun".to_owned()]
    };
    let manifest = TargetPackManifest {
        schema_version: TargetPackVersion::CURRENT,
        appd_version: env!("CARGO_PKG_VERSION").to_owned(),
        target,
        artifacts,
        required_tools,
    };
    write_manifest(pack_dir.join("target-pack.json"), &manifest)?;

    Ok(pack_dir.join("target-pack.json"))
}

fn android_bare_host_library(workspace: &Path) -> Result<PathBuf> {
    let inputs = workspace.join("target/bare/sdk/android-arm64/inputs");
    let host = fs::read_dir(&inputs)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("-libappd_bare.so"))
        });
    host.with_context(|| format!("Android Bare host library is missing: {}", inputs.display()))
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
    cargo.arg("--lib");
    if target == Target::AndroidArm64 {
        configure_android_toolchain(&mut cargo)?;
    } else {
        configure_apple_deployment_target(&mut cargo, target);
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
        Target::AndroidArm64 => unreachable!("Android has its own toolchain"),
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
    for file in ["AppdShell.swift", "AppdPlugins.swift"] {
        copy_file(
            workspace.join("crates/appd-shell-apple/native").join(file),
            destination.join(file),
        )?;
    }
    Ok(())
}

fn package_apple_framework(
    workspace: &Path,
    target: Target,
    rust_library: &Path,
    pack_dir: &Path,
) -> Result<()> {
    let framework = pack_dir.join("frameworks/AppdRuntime.framework");
    let object = pack_dir.join("appd-runtime.o");
    fs::create_dir_all(framework.join("Headers"))?;
    fs::create_dir_all(framework.join("Modules"))?;

    let mut clang = Command::new("xcrun");
    clang
        .args(["--sdk", apple_sdk(target), "clang", "-target"])
        .arg(apple_clang_target(target))
        .args(["-r", "-o"])
        .arg(&object)
        .arg(format!("-Wl,-force_load,{}", rust_library.display()))
        .args(bare_relocatable_link_args(workspace, target)?);
    let status = clang
        .current_dir(workspace)
        .status()
        .context("failed to combine the Apple runtime library")?;
    if !status.success() {
        bail!("Apple runtime library link failed with status {status}");
    }

    let status = Command::new("xcrun")
        .args(["libtool", "-static", "-o"])
        .arg(framework.join("AppdRuntime"))
        .arg(&object)
        .status()
        .context("failed to archive the Apple runtime framework")?;
    if !status.success() {
        bail!("Apple runtime framework archive failed with status {status}");
    }
    fs::remove_file(object)?;
    copy_file(
        workspace.join("crates/appd-shell-apple/native/AppdRuntime.h"),
        framework.join("Headers/AppdRuntime.h"),
    )?;
    copy_file(
        workspace.join("crates/appd-shell-apple/native/module.modulemap"),
        framework.join("Modules/module.modulemap"),
    )
}

fn bare_relocatable_link_args(workspace: &Path, target: Target) -> Result<Vec<String>> {
    let sdk = workspace.join("target/bare/sdk").join(target.to_string());
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(sdk.join("sdk-manifest.json"))?)?;
    let arguments = manifest["link_args"]
        .as_array()
        .context("Bare SDK manifest link_args must be an array")?;
    let mut result = Vec::new();
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        let argument = argument
            .as_str()
            .context("Bare SDK link argument must be a string")?;
        if argument == "-framework" {
            arguments.next();
            continue;
        }
        if matches!(
            argument,
            "-lSystem" | "-lpthread" | "-lm" | "-lresolv" | "-lc++"
        ) {
            continue;
        }
        result.push(argument.replace("inputs/", &format!("{}/inputs/", sdk.display())));
    }
    Ok(result)
}

fn apple_sdk(target: Target) -> &'static str {
    match target {
        Target::MacosArm64 | Target::MacosX64 => "macosx",
        Target::IosArm64 => "iphoneos",
        Target::IosSimulatorArm64 | Target::IosSimulatorX64 => "iphonesimulator",
        Target::AndroidArm64 => unreachable!("Android does not use the Apple SDK"),
    }
}

fn apple_clang_target(target: Target) -> &'static str {
    match target {
        Target::MacosArm64 => "arm64-apple-macos14.0",
        Target::MacosX64 => "x86_64-apple-macos14.0",
        Target::IosArm64 => "arm64-apple-ios17.0",
        Target::IosSimulatorArm64 => "arm64-apple-ios17.0-simulator",
        Target::IosSimulatorX64 => "x86_64-apple-ios17.0-simulator",
        Target::AndroidArm64 => unreachable!("Android does not use an Apple target"),
    }
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
    command.env("CC_aarch64_linux_android", &compiler);
    command.env("AR_aarch64_linux_android", host.join("bin/llvm-ar"));
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
    Ok(Artifact {
        kind,
        path: path.to_owned(),
        sha256: artifact_sha256(root.join(path))?,
    })
}

fn build_bare_sdk(workspace: &Path, target: Target) -> Result<()> {
    let sdk_target = target.to_string();
    let output = workspace.join("target/bare/sdk").join(&sdk_target);
    if env::var_os("APPD_BARE_SDK_DIR").is_some() && output.join("sdk-manifest.json").is_file() {
        return Ok(());
    }
    let status = Command::new(native_python())
        .arg("bare/scripts/build-sdk.py")
        .args(["--target", &sdk_target, "--output"])
        .arg(&output)
        .current_dir(workspace)
        .status()
        .context("failed to build the Bare SDK")?;
    if status.success() {
        Ok(())
    } else {
        bail!("Bare SDK build failed with status {status}")
    }
}

fn native_python() -> PathBuf {
    let homebrew = PathBuf::from("/opt/homebrew/bin/python3");
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") && homebrew.is_file() {
        homebrew
    } else {
        PathBuf::from("python3")
    }
}

fn build_runtime_tools(workspace: &Path) -> Result<()> {
    let script = "build:runtime";
    let status = Command::new("pnpm")
        .args(["run", script])
        .current_dir(workspace)
        .status()
        .with_context(|| format!("failed to run pnpm {script}"))?;
    if !status.success() {
        bail!("pnpm {script} failed with status {status}");
    }
    Ok(())
}

fn install_bare_tls(workspace: &Path, runtime: &Path, target: Target) -> Result<()> {
    let sdk_target = target.to_string();
    let host = match target {
        Target::AndroidArm64 => "android-arm64",
        Target::MacosArm64 => "darwin-arm64",
        Target::MacosX64 => "darwin-x64",
        Target::IosArm64 => "ios-arm64",
        Target::IosSimulatorArm64 => "ios-arm64-simulator",
        Target::IosSimulatorX64 => "ios-x64-simulator",
    };
    let source = workspace
        .join("target/bare/sdk")
        .join(sdk_target)
        .join("bare-tls.bare");
    let destination = runtime
        .join("node_modules/bare-tls/prebuilds")
        .join(host)
        .join("bare-tls.bare");
    copy_file(&source, destination)
}

fn deploy_runtime(workspace: &Path, output: &Path) -> Result<()> {
    clear_path(output)?;
    let status = Command::new("pnpm")
        .args([
            "--config.node-linker=isolated",
            "--filter",
            "appd",
            "deploy",
            "--prod",
        ])
        .arg(output)
        .current_dir(workspace)
        .status()
        .context("failed to deploy Bare runtime modules")?;
    if !status.success() {
        bail!("Bare runtime module deployment failed with status {status}")
    }
    copy_dir_contents(
        &workspace.join("target/runtime-js"),
        &output.join("runtime-js"),
    )
}

fn deploy_packer(workspace: &Path, output: &Path) -> Result<()> {
    clear_path(output)?;
    let status = Command::new("pnpm")
        .args([
            "--config.node-linker=isolated",
            "--filter",
            "@appd/bare-pack-tool",
            "deploy",
            "--prod",
        ])
        .arg(output)
        .current_dir(workspace)
        .status()
        .context("failed to deploy Bare bundle packer")?;
    if status.success() {
        Ok(())
    } else {
        bail!("Bare bundle packer deployment failed with status {status}")
    }
}

fn shell_package(target: Target) -> &'static str {
    if target == Target::AndroidArm64 {
        "appd-shell-android"
    } else {
        "appd-shell-apple"
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
    }
}

fn runtime_artifact_name(target: Target) -> &'static str {
    if target == Target::AndroidArm64 {
        ANDROID_RUNTIME_LIBRARY
    } else {
        APPLE_RUNTIME_LIBRARY
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
