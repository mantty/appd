from __future__ import annotations

import hashlib
import json
import os
import shlex
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib


BARE_ROOT = Path(__file__).resolve().parents[1]
APPD_ROOT = BARE_ROOT.parent
DEFAULT_TARGET_ROOT = APPD_ROOT / "target" / "bare"
DEFAULT_UPSTREAM_CONFIG = BARE_ROOT / "upstream.toml"
TARGETS = {
    "macos-arm64": ("darwin", "arm64"),
    "ios-arm64": ("ios", "arm64"),
}


def load_upstream_config(path: Path = DEFAULT_UPSTREAM_CONFIG) -> dict[str, str]:
    with path.open("rb") as file:
        data = tomllib.load(file)
    upstream = data.get("upstream")
    required = (
        "repository",
        "tag",
        "commit",
        "source_url",
        "source_sha256",
        "engine_repository",
        "engine_commit",
    )
    if not isinstance(upstream, dict):
        raise ValueError(f"{path} must contain an [upstream] table")
    missing = [name for name in required if not upstream.get(name)]
    if missing:
        raise ValueError(f"{path} is missing required keys: {', '.join(missing)}")
    return {name: str(upstream[name]) for name in required}


def fetch_upstream(target_root: Path = DEFAULT_TARGET_ROOT, force: bool = False) -> Path:
    upstream = load_upstream_config()
    source_dir = target_root / "src" / upstream["tag"]
    archive = target_root / "downloads" / f"{upstream['tag']}.tar.gz"
    if not source_dir.is_dir() or force:
        download_and_extract(upstream, archive, source_dir)
    install_upstream_dependencies(source_dir)
    return source_dir


def download_and_extract(upstream: dict[str, str], archive: Path, destination: Path) -> None:
    archive.parent.mkdir(parents=True, exist_ok=True)
    if not archive.is_file():
        urllib.request.urlretrieve(upstream["source_url"], archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != upstream["source_sha256"]:
        raise ValueError(f"checksum mismatch for {archive}")

    with tempfile.TemporaryDirectory(dir=archive.parent) as temporary:
        root = Path(temporary) / "source"
        root.mkdir()
        with tarfile.open(archive, "r:gz") as tar:
            safe_extract(tar, root)
        children = [path for path in root.iterdir() if path.is_dir()]
        if len(children) != 1:
            raise ValueError(f"expected one source directory in {archive}")
        if destination.exists():
            shutil.rmtree(destination)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(children[0]), destination)


def safe_extract(archive: tarfile.TarFile, destination: Path) -> None:
    root = destination.resolve()
    for member in archive.getmembers():
        target = (destination / member.name).resolve()
        if target != root and root not in target.parents:
            raise ValueError(f"archive member escapes destination: {member.name}")
    archive.extractall(destination)


def install_upstream_dependencies(source_dir: Path) -> None:
    modules = source_dir / "node_modules"
    lock = source_dir / "package-lock.json"
    digest = hashlib.sha256(lock.read_bytes()).hexdigest()
    stamp = modules / ".appd-package-lock"
    if stamp.is_file() and stamp.read_text().strip() == digest:
        return
    if modules.exists() or modules.is_symlink():
        if modules.is_dir() and not modules.is_symlink():
            shutil.rmtree(modules)
        else:
            modules.unlink()
    subprocess.run(["npm", "ci", "--ignore-scripts"], cwd=source_dir, check=True)
    stamp.write_text(digest + "\n")


def build_sdk(target: str, output: Path, target_root: Path = DEFAULT_TARGET_ROOT) -> None:
    platform, arch = target_settings(target)
    source = fetch_upstream(target_root)
    build = target_root / "build" / target
    generate(build, source, platform, arch)
    run_bare_make("build", "--build", str(build), "--target", "appd_bare_link_test")
    bare_tls = build_bare_tls(build)
    package_sdk(build, output, target)
    shutil.copy2(bare_tls, output / "bare-tls.bare")


def target_settings(target: str) -> tuple[str, str]:
    try:
        return TARGETS[target]
    except KeyError as error:
        raise ValueError(f"unsupported Bare target: {target}") from error


def generate(build: Path, source: Path, platform: str, arch: str) -> None:
    upstream = load_upstream_config()
    engine = f"{upstream['engine_repository']}#{upstream['engine_commit']}"
    arguments = [
        "generate",
        "--source",
        str(BARE_ROOT),
        "--build",
        str(build),
        "--platform",
        platform,
        "--arch",
        arch,
        "--with-minimal-size",
        "--define",
        f"BARE_KIT_SOURCE:PATH={source}",
        "--define",
        f"BARE_ENGINE:STRING={engine}",
    ]
    if platform == "ios":
        arguments.extend(
            (
                "--define",
                "CMAKE_OSX_DEPLOYMENT_TARGET:STRING=17.0",
            )
        )
    launcher = os.environ.get("SCCACHE") or shutil.which("sccache")
    if launcher:
        for language in ("C", "CXX", "OBJC", "OBJCXX"):
            arguments.extend(("--define", f"CMAKE_{language}_COMPILER_LAUNCHER:FILEPATH={launcher}"))
    run_bare_make(*arguments)


def run_bare_make(*arguments: str) -> None:
    command = ["pnpm", "bare-make", *arguments]
    subprocess.run(command, cwd=APPD_ROOT, check=True)


def build_bare_tls(build: Path) -> Path:
    ninja = cmake_cache_value(build / "CMakeCache.txt", "CMAKE_MAKE_PROGRAM")
    result = subprocess.run(
        [ninja, "-C", str(build), "-t", "targets"],
        check=True,
        capture_output=True,
        text=True,
    )
    target_name = next(
        line.split(":", 1)[0]
        for line in result.stdout.splitlines()
        if line.startswith("bare-tls-") and line.endswith("_module: phony")
    )
    subprocess.run([ninja, "-C", str(build), target_name], check=True)
    artifact = build / "node_modules/bare-tls/bare-tls@3.bare"
    return artifact


def package_sdk(build: Path, output: Path, target: str) -> None:
    command = link_command(build)
    arguments = force_load_appd_bare(link_arguments(command)) + driver_link_arguments(target)
    if output.exists():
        shutil.rmtree(output)
    inputs = copy_link_inputs(build, output, arguments)
    upstream = load_upstream_config()
    module_lock = hashlib.sha256((APPD_ROOT / "pnpm-lock.yaml").read_bytes()).hexdigest()
    manifest = {
        "schema_version": 1,
        "target": target,
        "upstream": {"tag": upstream["tag"], "commit": upstream["commit"]},
        "engine": {
            "repository": upstream["engine_repository"],
            "commit": upstream["engine_commit"],
        },
        "module_lock_sha256": module_lock,
        "link_args": rewrite_link_arguments(build, inputs, arguments),
        "link_inputs": [{"path": relative} for _, relative in inputs],
    }
    output.mkdir(parents=True, exist_ok=True)
    (output / "sdk-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    shutil.copy2(BARE_ROOT / "include" / "appd_bare.h", output / "appd_bare.h")


def driver_link_arguments(target: str) -> list[str]:
    target_settings(target)
    return ["-lc++"]


def link_command(build: Path) -> str:
    ninja = cmake_cache_value(build / "CMakeCache.txt", "CMAKE_MAKE_PROGRAM")
    result = subprocess.run(
        [ninja, "-C", str(build), "-t", "commands", "appd_bare_link_test"],
        check=True,
        capture_output=True,
        text=True,
    )
    commands = [line for line in result.stdout.splitlines() if line.strip()]
    return commands[-1]


def cmake_cache_value(cache: Path, name: str) -> str:
    prefix = f"{name}:"
    for line in cache.read_text().splitlines():
        if line.startswith(prefix):
            return line.split("=", 1)[1]
    raise ValueError(f"{name} is missing from {cache}")


def link_arguments(command: str) -> list[str]:
    tokens = shlex.split(command)
    output_index = tokens.index("-o")
    arguments = tokens[output_index + 2 :]
    return arguments[: arguments.index("&&")] if "&&" in arguments else arguments


def force_load_appd_bare(arguments: list[str]) -> list[str]:
    for index, argument in enumerate(arguments):
        if Path(argument).name == "libappd_bare.a":
            return arguments[:index] + ["-Xlinker", "-force_load", "-Xlinker", argument] + arguments[index + 1 :]
    raise ValueError("link command does not include libappd_bare.a")


def copy_link_inputs(
    build: Path,
    output: Path,
    arguments: list[str],
) -> list[tuple[Path, str]]:
    inputs: list[tuple[Path, str]] = []
    seen: set[Path] = set()
    for argument in arguments:
        source = (build / argument).resolve()
        if source in seen or not source.is_file() or source.suffix not in {".a", ".o"}:
            continue
        seen.add(source)
        relative = f"inputs/{len(inputs):04d}-{source.name}"
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        inputs.append((source, relative))
    return inputs


def rewrite_link_arguments(
    build: Path,
    inputs: list[tuple[Path, str]],
    arguments: list[str],
) -> list[str]:
    paths = {source: relative for source, relative in inputs}
    rewritten = []
    for argument in arguments:
        source = (build / argument).resolve()
        rewritten.append(paths.get(source, argument))
    return rewritten
