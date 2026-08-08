use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use appd_target_pack::{
    Artifact, ArtifactKind, Target, TargetPackManifest, TargetPackVersion, write_manifest,
};

use crate::BuildPlatform;
use crate::build::support::{
    copy_dir_contents, copy_dir_contents_preserving_symlinks, copy_file, package_manager,
};

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
    build_bare_sdk(workspace, target)?;
    build_runtime_tools(workspace)?;
    let library = build_runtime_library(workspace, target, rust_target)?;
    let (runtime_kind, runtime_path) = package_runtime(workspace, target, &library, &pack_dir)?;
    install_bare_runtime(workspace, target, &pack_dir)?;
    if target != Target::WindowsX64 {
        install_native_shell(workspace, target, &pack_dir)?;
    }
    let tools_dir = pack_dir.join("tools");
    deploy_runtime(workspace, &tools_dir.join("runtime"))?;
    deploy_packer(workspace, &tools_dir.join("packer"))?;
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

fn install_bare_runtime(workspace: &Path, target: Target, pack_dir: &Path) -> Result<()> {
    copy_dir_contents_preserving_symlinks(
        &workspace
            .join("target/bare/sdk")
            .join(target.to_string())
            .join("runtime"),
        &pack_dir.join("bare-runtime"),
    )?;
    copy_file(
        workspace
            .join("target/bare/sdk")
            .join(target.to_string())
            .join("builtins.json"),
        pack_dir.join("bare-runtime/builtins.json"),
    )?;
    Ok(())
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
            esbuild_executable(target),
        )?,
        packaged_artifact(pack_dir, ArtifactKind::BareRuntimeDirectory, "bare-runtime")?,
    ]);
    let packer = bare_pack_executable(pack_dir)?;
    artifacts.push(packaged_artifact(
        pack_dir,
        ArtifactKind::BarePackExecutable,
        &packer,
    )?);
    Ok(artifacts)
}

fn bare_pack_executable(pack_dir: &Path) -> Result<String> {
    let root = fs::canonicalize(pack_dir)?;
    let path = fs::canonicalize(root.join("tools/packer/node_modules/bare-pack/bin.js"))?;
    let path = path
        .strip_prefix(&root)
        .context("Bare Pack executable is outside its target pack")?;
    Ok(path.to_string_lossy().replace('\\', "/"))
}

fn esbuild_executable(target: Target) -> &'static str {
    if target == Target::WindowsX64 {
        "tools/packer/node_modules/@esbuild/win32-x64/esbuild.exe"
    } else {
        "tools/packer/node_modules/esbuild/bin/esbuild"
    }
}

fn required_tools(target: Target) -> Vec<String> {
    match target {
        Target::AndroidArm64 => vec!["node".to_owned(), "gradle".to_owned()],
        Target::WindowsX64 => vec!["node".to_owned()],
        _ => vec!["node".to_owned(), "xcrun".to_owned()],
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

fn build_bare_sdk(workspace: &Path, target: Target) -> Result<()> {
    let sdk_target = target.to_string();
    let output = workspace.join("target/bare/sdk").join(&sdk_target);
    if env::var_os("APPD_BARE_SDK_DIR").is_some() && output.join("sdk-manifest.json").is_file() {
        return Ok(());
    }
    let bare = workspace.join("bare");
    let target_root = workspace.join("target/bare");
    let modules = target_root.join("modules").join(&sdk_target);
    let build = target_root.join("build").join(&sdk_target);

    deploy_bare_modules(workspace, &modules)?;
    generate_bare_runtime(&bare, &build, &modules, target)?;
    run_bare_make(
        &bare,
        [
            "build",
            "--build",
            build.to_string_lossy().as_ref(),
            "--target",
            "appd_bare_kit",
        ],
    )?;

    reset_dir(&output)?;
    let runtime = build.join("runtime");
    if target == Target::AndroidArm64 {
        copy_file(
            runtime.join("libbare-kit.so"),
            output.join("runtime/libbare-kit.so"),
        )?;
    } else if target == Target::WindowsX64 {
        copy_dir_contents(&runtime, &output.join("runtime"))?;
    } else {
        copy_dir_contents_preserving_symlinks(
            &runtime.join("BareKit.framework"),
            &output.join("runtime/BareKit.framework"),
        )?;
    }
    write_builtins(&modules, &output.join("builtins.json"))?;
    fs::write(
        output.join("sdk-manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 4,
            "target": sdk_target,
        }))?,
    )?;
    Ok(())
}

fn write_builtins(modules: &Path, output: &Path) -> Result<()> {
    let mut builtins = Vec::new();
    for entry in fs::read_dir(modules.join("node_modules"))? {
        let package = entry?.path().join("package.json");
        let Ok(source) = fs::read_to_string(package) else {
            continue;
        };
        let value: serde_json::Value = serde_json::from_str(&source)?;
        if !value
            .get("addon")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let name = value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .context("Bare addon package is missing its name")?;
        builtins.push(name.to_owned());
    }
    builtins.sort_unstable();
    if builtins.is_empty() {
        bail!("no Bare native modules were deployed")
    }
    let builtins = builtins
        .into_iter()
        .map(|addon| serde_json::json!({ "addon": addon }))
        .collect::<Vec<_>>();
    fs::write(output, serde_json::to_vec_pretty(&builtins)?)?;
    Ok(())
}

fn deploy_bare_modules(workspace: &Path, output: &Path) -> Result<()> {
    reset_dir(output)?;
    let status = Command::new(package_manager("pnpm"))
        .args([
            "--config.node-linker=isolated",
            "--filter",
            "@appd/bare-runtime",
            "deploy",
            "--prod",
        ])
        .arg(output)
        .current_dir(workspace)
        .status()
        .context("failed to deploy Bare runtime modules")?;
    if status.success() {
        Ok(())
    } else {
        bail!("Bare runtime module deployment failed with status {status}")
    }
}

fn generate_bare_runtime(bare: &Path, build: &Path, modules: &Path, target: Target) -> Result<()> {
    let mut arguments = vec![
        "generate".to_owned(),
        "--source".to_owned(),
        bare.to_string_lossy().into_owned(),
        "--build".to_owned(),
        build.to_string_lossy().into_owned(),
        "--platform".to_owned(),
        bare_platform(target).to_owned(),
        "--arch".to_owned(),
        bare_arch(target).to_owned(),
        "--with-minimal-size".to_owned(),
        "--define".to_owned(),
        format!("APPD_BARE_MODULES_ROOT:PATH={}", modules.display()),
    ];
    if is_simulator(target) {
        arguments.push("--simulator".to_owned());
    }
    if matches!(
        target,
        Target::MacosArm64
            | Target::MacosX64
            | Target::IosArm64
            | Target::IosSimulatorArm64
            | Target::IosSimulatorX64
    ) {
        arguments.extend(["--define".to_owned(), "APPLE_CLANG:BOOL=ON".to_owned()]);
    }
    if matches!(
        target,
        Target::IosArm64 | Target::IosSimulatorArm64 | Target::IosSimulatorX64
    ) {
        arguments.extend([
            "--define".to_owned(),
            "CMAKE_OSX_DEPLOYMENT_TARGET:STRING=17.0".to_owned(),
        ]);
    }
    if target == Target::AndroidArm64 {
        arguments.extend([
            "--define".to_owned(),
            "ANDROID_PLATFORM:STRING=android-31".to_owned(),
            "--define".to_owned(),
            "ANDROID_STL:STRING=c++_static".to_owned(),
        ]);
    }
    add_compiler_cache(&mut arguments);
    run_bare_make(bare, arguments.iter().map(String::as_str))
}

fn run_bare_make<'a>(bare: &Path, arguments: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let status = Command::new(package_manager("pnpm"))
        .args(["--filter", "@appd/bare-sdk", "exec", "bare-make"])
        .args(arguments)
        .current_dir(bare.parent().context("bare directory must have a parent")?)
        .status()
        .context("failed to run bare-make")?;
    if status.success() {
        Ok(())
    } else {
        bail!("bare-make failed with status {status}")
    }
}

fn add_compiler_cache(arguments: &mut Vec<String>) {
    let Some(launcher) = env::var_os("SCCACHE") else {
        return;
    };
    for language in ["C", "CXX", "OBJC", "OBJCXX"] {
        arguments.extend([
            "--define".to_owned(),
            format!(
                "CMAKE_{language}_COMPILER_LAUNCHER:FILEPATH={}",
                launcher.to_string_lossy()
            ),
        ]);
    }
}

fn bare_platform(target: Target) -> &'static str {
    if target == Target::AndroidArm64 {
        "android"
    } else if matches!(target, Target::MacosArm64 | Target::MacosX64) {
        "darwin"
    } else if target == Target::WindowsX64 {
        "win32"
    } else {
        "ios"
    }
}

fn bare_arch(target: Target) -> &'static str {
    match target {
        Target::MacosX64 | Target::IosSimulatorX64 | Target::WindowsX64 => "x64",
        _ => "arm64",
    }
}

fn is_simulator(target: Target) -> bool {
    matches!(target, Target::IosSimulatorArm64 | Target::IosSimulatorX64)
}

fn build_runtime_tools(workspace: &Path) -> Result<()> {
    let script = "build:runtime";
    let status = Command::new(package_manager("pnpm"))
        .args(["run", script])
        .current_dir(workspace)
        .status()
        .with_context(|| format!("failed to run pnpm {script}"))?;
    if !status.success() {
        bail!("pnpm {script} failed with status {status}");
    }
    Ok(())
}

fn deploy_runtime(workspace: &Path, output: &Path) -> Result<()> {
    clear_path(output)?;
    let status = Command::new(package_manager("pnpm"))
        .args([
            "--config.node-linker=isolated",
            "--filter",
            "@appd/bare-runtime",
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
    remove_runtime_self_link(output)?;
    copy_dir_contents(
        &workspace.join("target/runtime-js"),
        &output.join("runtime-js"),
    )
}

fn remove_runtime_self_link(output: &Path) -> Result<()> {
    let link = output.join("node_modules/@appd/bare-runtime");
    if fs::symlink_metadata(&link).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        fs::remove_dir_all(link)?;
    }
    Ok(())
}

fn deploy_packer(workspace: &Path, output: &Path) -> Result<()> {
    clear_path(output)?;
    let status = Command::new(package_manager("pnpm"))
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
    use super::{
        bare_pack_executable, esbuild_executable, packaged_artifact, remove_runtime_self_link,
        write_builtins,
    };
    use appd_target_pack::{ArtifactKind, Target};
    use std::fs;

    #[test]
    fn writes_native_modules_as_builtins() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let modules = directory.path().join("node_modules");
        fs::create_dir_all(modules.join("native"))?;
        fs::create_dir_all(modules.join("javascript"))?;
        fs::write(
            modules.join("native/package.json"),
            r#"{"name":"native","addon":true}"#,
        )?;
        fs::write(
            modules.join("javascript/package.json"),
            r#"{"name":"javascript"}"#,
        )?;

        let output = directory.path().join("builtins.json");
        write_builtins(directory.path(), &output)?;

        assert_eq!(
            fs::read_to_string(output)?,
            "[\n  {\n    \"addon\": \"native\"\n  }\n]"
        );
        Ok(())
    }

    #[test]
    fn uses_the_native_windows_esbuild_binary() {
        assert_eq!(
            esbuild_executable(Target::WindowsX64),
            "tools/packer/node_modules/@esbuild/win32-x64/esbuild.exe"
        );
    }

    #[test]
    fn rejects_missing_pack_artifacts() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let result =
            packaged_artifact(directory.path(), ArtifactKind::BareRuntimeDirectory, "bare");

        assert!(result.is_err_and(|error| {
            error
                .to_string()
                .contains("target-pack artifact was not produced")
        }));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn removes_the_deployed_runtime_self_link() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let appd = directory.path().join("node_modules/@appd");
        let link = appd.join("bare-runtime");
        fs::create_dir_all(appd)?;
        symlink("../../../../bare/runtime", &link)?;

        remove_runtime_self_link(directory.path())?;

        assert!(!link.exists());
        assert!(fs::symlink_metadata(link).is_err());
        Ok(())
    }

    #[test]
    fn keeps_a_deployed_runtime_directory() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let runtime = directory.path().join("node_modules/@appd/bare-runtime");
        fs::create_dir_all(&runtime)?;

        remove_runtime_self_link(directory.path())?;

        assert!(runtime.is_dir());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn resolves_bare_pack_executable_through_pnpm_links() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let pack_dir = directory.path().join("pack");
        let target_dir =
            pack_dir.join("tools/packer/node_modules/.pnpm/bare-pack@1/node_modules/bare-pack");
        fs::create_dir_all(&target_dir)?;
        let target = target_dir.join("bin.js");
        fs::write(&target, "#!/usr/bin/env node\n")?;

        let link_dir = pack_dir.join("tools/packer/node_modules/bare-pack");
        fs::create_dir_all(&link_dir)?;
        let link = link_dir.join("bin.js");
        symlink(&target, &link)?;

        assert_eq!(
            bare_pack_executable(&pack_dir)?,
            "tools/packer/node_modules/.pnpm/bare-pack@1/node_modules/bare-pack/bin.js"
        );
        Ok(())
    }
}
