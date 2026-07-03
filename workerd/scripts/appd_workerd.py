from __future__ import annotations

import contextlib
import dataclasses
import hashlib
import json
import os
import platform
import shutil
import shlex
import socket
import subprocess
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Iterable, Iterator

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    import tomli as tomllib


WORKERD_ROOT = Path(__file__).resolve().parents[1]
APPD_ROOT = WORKERD_ROOT.parent
DEFAULT_TARGET_ROOT = APPD_ROOT / "target" / "workerd"
DEFAULT_CACHE_ROOT = DEFAULT_TARGET_ROOT / "cache"
DEFAULT_UPSTREAM_CONFIG = WORKERD_ROOT / "upstream.toml"
DEFAULT_OVERLAY_ROOT = WORKERD_ROOT / "overlay"
DEFAULT_PATCHES_ROOT = WORKERD_ROOT / "patches"
APPD_BAZEL_TARGET = "//src/workerd/server:appd-workerd-link-anchor"
LINK_SUFFIXES = (".a", ".lib", ".lo", ".o", ".obj", ".rlib")
TARGET_ALIASES = {
    "linux-x64": "x86_64-unknown-linux-gnu",
    "macos-arm64": "aarch64-apple-darwin",
    "macos-x64": "x86_64-apple-darwin",
    "windows-x64": "x86_64-pc-windows-msvc",
}
DEFAULT_BAZEL_ARGS_BY_TARGET = {
    "x86_64-unknown-linux-gnu": ["--config=release_linux"],
    "aarch64-apple-darwin": ["--config=release_macos"],
    "x86_64-pc-windows-msvc": ["--config=release_windows"],
}
CACHE_MODES = ("off", "local", "r2-read", "r2-read-write")
DEFAULT_R2_ACCOUNT_ID = "dacf3ead71e534fdef9555c28d81774c"
DEFAULT_R2_BUCKET = "appd-workerd-bazel-cache"
DEFAULT_R2_PREFIX = "shared-cache"
DEFAULT_BAZEL_REMOTE_MAX_SIZE_GIB = 8
# Pinned to match the same install command CI uses (weekly workflow), so
# local dev and CI run the identical bazel-remote build, not just the same
# version number.
BAZEL_REMOTE_VERSION = "v2.6.1"
BAZEL_REMOTE_MODULE = f"github.com/buchgr/bazel-remote/v2@{BAZEL_REMOTE_VERSION}"


@dataclasses.dataclass(frozen=True)
class R2CacheSettings:
    endpoint: str
    bucket: str
    prefix: str
    access_key_id: str
    secret_access_key: str
    session_token: str = ""


@dataclasses.dataclass(frozen=True)
class BuildCacheConfig:
    mode: str = "local"
    cache_dir: Path = DEFAULT_CACHE_ROOT
    bazel_remote_bin: str | None = None
    bazel_remote_port: int = 0
    max_size_gib: int = DEFAULT_BAZEL_REMOTE_MAX_SIZE_GIB
    r2: R2CacheSettings | None = None

    def __post_init__(self) -> None:
        if self.mode not in CACHE_MODES:
            modes = ", ".join(CACHE_MODES)
            raise ValueError(f"unsupported workerd cache mode {self.mode}; supported modes: {modes}")
        if self.mode.startswith("r2-") and self.r2 is None:
            raise ValueError(f"{self.mode} requires R2 cache settings")
        if self.bazel_remote_port < 0 or self.bazel_remote_port > 65535:
            raise ValueError("bazel-remote port must be between 0 and 65535")
        if self.max_size_gib <= 0:
            raise ValueError("bazel-remote max size must be positive")


def load_upstream_config(config_path: Path = DEFAULT_UPSTREAM_CONFIG) -> dict[str, str]:
    with config_path.open("rb") as file:
        data = tomllib.load(file)

    upstream = data.get("upstream")
    if not isinstance(upstream, dict):
        raise ValueError(f"{config_path} must contain an [upstream] table")

    required = ("repository", "tag", "commit", "source_url", "source_sha256")
    missing = [key for key in required if not upstream.get(key)]
    if missing:
        joined = ", ".join(missing)
        raise ValueError(f"{config_path} is missing required upstream keys: {joined}")

    return {key: str(upstream[key]) for key in required}


def fetch_upstream(
    config_path: Path = DEFAULT_UPSTREAM_CONFIG,
    target_root: Path = DEFAULT_TARGET_ROOT,
    force: bool = False,
    refresh: bool = False,
) -> Path:
    upstream = load_upstream_config(config_path)
    tag = upstream["tag"]
    source_dir = target_root / "src" / tag
    archive_path = target_root / "downloads" / f"{tag}.tar.gz"

    if source_dir.is_dir() and not force:
        return source_dir

    archive_path.parent.mkdir(parents=True, exist_ok=True)
    if refresh or not archive_path.is_file():
        urllib.request.urlretrieve(upstream["source_url"], archive_path)

    actual_sha = sha256_file(archive_path)
    if actual_sha != upstream["source_sha256"]:
        raise ValueError(
            f"checksum mismatch for {archive_path}: expected "
            f"{upstream['source_sha256']}, got {actual_sha}"
        )

    with tempfile.TemporaryDirectory(dir=target_root) as temp_dir:
        extract_root = Path(temp_dir) / "extract"
        extract_root.mkdir()
        with tarfile.open(archive_path, "r:gz") as archive:
            safe_extract(archive, extract_root)

        children = [child for child in extract_root.iterdir() if child.is_dir()]
        if len(children) != 1:
            raise ValueError(f"expected one top-level source directory in {archive_path}")

        source_dir.parent.mkdir(parents=True, exist_ok=True)
        if source_dir.exists():
            shutil.rmtree(source_dir)
        shutil.move(str(children[0]), source_dir)

    return source_dir


def safe_extract(archive: tarfile.TarFile, destination: Path) -> None:
    destination = destination.resolve()
    for member in archive.getmembers():
        target = (destination / member.name).resolve()
        if not str(target).startswith(str(destination) + "/") and target != destination:
            raise ValueError(f"tar archive member escapes destination: {member.name}")
    archive.extractall(destination)


def apply_overlay(
    source_dir: Path,
    overlay_root: Path = DEFAULT_OVERLAY_ROOT,
    patches_root: Path = DEFAULT_PATCHES_ROOT,
) -> None:
    copy_overlay_files(source_dir, overlay_root)
    apply_patches(source_dir, patches_root)


def copy_overlay_files(source_dir: Path, overlay_root: Path) -> None:
    if not overlay_root.is_dir():
        raise FileNotFoundError(f"overlay directory does not exist: {overlay_root}")

    for path in sorted(overlay_root.rglob("*")):
        relative = path.relative_to(overlay_root)
        target = source_dir / relative
        if path.is_dir():
            target.mkdir(parents=True, exist_ok=True)
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)


def apply_patches(source_dir: Path, patches_root: Path) -> None:
    if not patches_root.is_dir():
        return

    for patch_path in sorted(patches_root.glob("*.patch")):
        if patch_is_already_applied(source_dir, patch_path):
            continue

        result = subprocess.run(
            ["git", "apply", "--check", str(patch_path)],
            cwd=source_dir,
            check=False,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"patch check failed for {patch_path}\n"
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
            )
        subprocess.run(["git", "apply", str(patch_path)], cwd=source_dir, check=True)


def patch_is_already_applied(source_dir: Path, patch_path: Path) -> bool:
    result = subprocess.run(
        ["git", "apply", "--reverse", "--check", str(patch_path)],
        cwd=source_dir,
        check=False,
        capture_output=True,
        text=True,
    )
    return result.returncode == 0


def package_sdk(
    *,
    params_path: Path,
    output_dir: Path,
    target: str,
    upstream_tag: str,
    upstream_commit: str,
    header_path: Path,
) -> Path:
    tokens = reusable_link_tokens(parse_params(params_path))
    if output_dir.exists():
        shutil.rmtree(output_dir)

    include_dir = output_dir / "include"
    lib_dir = output_dir / "lib"
    include_dir.mkdir(parents=True, exist_ok=True)
    lib_dir.mkdir(parents=True, exist_ok=True)

    shutil.copy2(header_path, include_dir / "appd_workerd.h")

    copied: dict[Path, str] = {}
    link_inputs: list[dict[str, str | int]] = []
    link_args: list[str] = []

    for token in tokens:
        candidates = list(link_input_candidates(token))
        if any(is_link_anchor(candidate) for candidate in candidates):
            continue

        rewritten = token
        for candidate in candidates:
            source = resolve_link_input(candidate, params_path.parent)
            packaged_path = copied.get(source)
            if packaged_path is None:
                packaged_path = copy_link_input(source, lib_dir, len(copied))
                copied[source] = packaged_path
                link_inputs.append(
                    {
                        "path": packaged_path,
                        "source": str(source),
                        "sha256": sha256_file(output_dir / packaged_path),
                        "bytes": (output_dir / packaged_path).stat().st_size,
                    }
                )
            rewritten = rewritten.replace(candidate, packaged_path)

        link_args.append(rewritten)

    manifest = {
        "schema_version": 1,
        "target": target,
        "upstream": {
            "tag": upstream_tag,
            "commit": upstream_commit,
        },
        "include": "include/appd_workerd.h",
        "link_inputs": link_inputs,
        "link_args": link_args,
    }

    manifest_path = output_dir / "sdk-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return manifest_path


def parse_params(params_path: Path) -> list[str]:
    tokens: list[str] = []
    for line in params_path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped:
            tokens.extend(shlex.split(stripped))
    return tokens


def reusable_link_tokens(tokens: list[str]) -> list[str]:
    reusable: list[str] = []
    index = 0
    while index < len(tokens):
        token = tokens[index]

        if token == "-o":
            index += 2
            continue
        if token.startswith("LINKED_BINARY="):
            index += 1
            continue
        if token == "-target":
            index += 2
            continue
        if token in {"-no-canonical-prefixes", "-Wl,-oso_prefix,__BAZEL_EXECUTION_ROOT__/"}:
            index += 1
            continue
        if token == "-Xlinker" and index + 1 < len(tokens) and tokens[index + 1] == "-object_path_lto":
            index = skip_xlinker_object_path_lto(tokens, index)
            continue
        if token == "-object_path_lto":
            index += 2
            continue

        reusable.append(token)
        index += 1

    return reusable


def skip_xlinker_object_path_lto(tokens: list[str], index: int) -> int:
    index += 2
    if index < len(tokens) and tokens[index] == "-Xlinker":
        index += 1
    if index < len(tokens):
        index += 1
    return index


def link_input_candidates(token: str) -> Iterable[str]:
    for fragment in token.split(","):
        if fragment.endswith(LINK_SUFFIXES):
            yield fragment


def is_link_anchor(path_text: str) -> bool:
    name = Path(path_text).name
    return "appd_workerd_link_anchor" in name or "appd-workerd-link-anchor" in name


def resolve_link_input(path_text: str, base_dir: Path) -> Path:
    path = Path(path_text)
    for candidate in link_input_resolution_candidates(path, base_dir):
        resolved = candidate.resolve()
        if resolved.is_file():
            return resolved
    raise FileNotFoundError(f"link input does not exist: {path_text}")


def link_input_resolution_candidates(path: Path, base_dir: Path) -> Iterable[Path]:
    if path.is_absolute():
        yield path
        return

    yield base_dir / path

    workspace_root = find_bazel_workspace_root(base_dir)
    if workspace_root is not None:
        yield workspace_root / path


def find_bazel_workspace_root(start: Path) -> Path | None:
    for path in (start, *start.parents):
        if (path / "MODULE.bazel").is_file() or (path / "WORKSPACE").exists():
            return path
    return None


def copy_link_input(source: Path, lib_dir: Path, index: int) -> str:
    name = f"{index:04d}-{sanitize_filename(source.name)}"
    target = lib_dir / name
    shutil.copy2(source, target)
    return f"lib/{name}"


def sanitize_filename(name: str) -> str:
    return "".join(char if char.isalnum() or char in "._-" else "_" for char in name)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def normalize_target(target: str) -> str:
    normalized = TARGET_ALIASES.get(target, target)
    if normalized in DEFAULT_BAZEL_ARGS_BY_TARGET or normalized == "x86_64-apple-darwin":
        return normalized

    supported = ", ".join(sorted(set(DEFAULT_BAZEL_ARGS_BY_TARGET) | {"x86_64-apple-darwin"}))
    aliases = ", ".join(sorted(TARGET_ALIASES))
    raise ValueError(
        f"no default Bazel configuration is defined for {target}; "
        f"supported targets: {supported}; aliases: {aliases}"
    )


def default_bazel_args(
    target: str,
    *,
    host_system: str | None = None,
    host_machine: str | None = None,
) -> list[str]:
    target = normalize_target(target)
    if target == "x86_64-apple-darwin":
        system = host_system or platform.system()
        machine = (host_machine or platform.machine()).lower()
        if system == "Darwin" and machine in {"arm64", "aarch64"}:
            return ["--config=release_macos_cross_x86_64"]
        return ["--config=release_macos"]

    return list(DEFAULT_BAZEL_ARGS_BY_TARGET[target])


def cache_config_from_env(
    *,
    mode: str | None = None,
    cache_dir: Path | None = None,
    bazel_remote_bin: str | None = None,
    bazel_remote_port: int | None = None,
    max_size_gib: int | None = None,
    r2_endpoint: str | None = None,
    r2_account_id: str | None = None,
    r2_bucket: str | None = None,
    r2_prefix: str | None = None,
    r2_access_key_id: str | None = None,
    r2_secret_access_key: str | None = None,
    r2_session_token: str | None = None,
    env: dict[str, str] | None = None,
) -> BuildCacheConfig:
    env = env or os.environ
    resolved_mode = mode or env.get("APPD_WORKERD_CACHE") or default_cache_mode(env)
    resolved_cache_dir = cache_dir or Path(env.get("APPD_WORKERD_CACHE_DIR", DEFAULT_CACHE_ROOT))
    resolved_bazel_remote_bin = bazel_remote_bin or env.get("APPD_BAZEL_REMOTE_BIN")
    resolved_bazel_remote_port = bazel_remote_port
    if resolved_bazel_remote_port is None:
        resolved_bazel_remote_port = int(env.get("APPD_BAZEL_REMOTE_PORT", "0"))
    resolved_max_size_gib = max_size_gib
    if resolved_max_size_gib is None:
        resolved_max_size_gib = int(
            env.get("APPD_BAZEL_REMOTE_MAX_SIZE_GIB", str(DEFAULT_BAZEL_REMOTE_MAX_SIZE_GIB))
        )

    return BuildCacheConfig(
        mode=resolved_mode,
        cache_dir=resolved_cache_dir,
        bazel_remote_bin=resolved_bazel_remote_bin,
        bazel_remote_port=resolved_bazel_remote_port,
        max_size_gib=resolved_max_size_gib,
        r2=r2_settings_from_env(
            endpoint=r2_endpoint,
            account_id=r2_account_id,
            bucket=r2_bucket,
            prefix=r2_prefix,
            access_key_id=r2_access_key_id,
            secret_access_key=r2_secret_access_key,
            session_token=r2_session_token,
            env=env,
        )
        if resolved_mode.startswith("r2-")
        else None,
    )


def default_cache_mode(env: dict[str, str]) -> str:
    if complete_r2_environment(env):
        return "r2-read-write"
    raise ValueError(
        "APPD_BAZEL_S3_ACCESS_KEY_ID and APPD_BAZEL_S3_SECRET_ACCESS_KEY are not "
        "set, so the shared remote cache can't be used. Set both to build with it "
        "(this is the default and gives dramatically faster builds), or pass "
        "--cache local (or --cache off) to build without it on purpose."
    )


def complete_r2_environment(env: dict[str, str]) -> bool:
    access_key_id = env.get("APPD_BAZEL_S3_ACCESS_KEY_ID")
    secret_access_key = env.get("APPD_BAZEL_S3_SECRET_ACCESS_KEY")
    return bool(access_key_id and secret_access_key)


def r2_settings_from_env(
    *,
    endpoint: str | None = None,
    account_id: str | None = None,
    bucket: str | None = None,
    prefix: str | None = None,
    access_key_id: str | None = None,
    secret_access_key: str | None = None,
    session_token: str | None = None,
    env: dict[str, str] | None = None,
) -> R2CacheSettings:
    env = env or os.environ
    resolved_endpoint = endpoint
    resolved_account_id = account_id or DEFAULT_R2_ACCOUNT_ID
    if not resolved_endpoint:
        # bazel-remote's --s3.endpoint wants a bare hostname, no scheme.
        resolved_endpoint = f"{resolved_account_id}.r2.cloudflarestorage.com"
    resolved_access_key_id = (
        access_key_id or env.get("APPD_BAZEL_S3_ACCESS_KEY_ID", "")
    )
    resolved_secret_access_key = (
        secret_access_key or env.get("APPD_BAZEL_S3_SECRET_ACCESS_KEY", "")
    )

    settings = R2CacheSettings(
        endpoint=resolved_endpoint or "",
        bucket=bucket or DEFAULT_R2_BUCKET,
        prefix=prefix or DEFAULT_R2_PREFIX,
        access_key_id=resolved_access_key_id,
        secret_access_key=resolved_secret_access_key,
        session_token=session_token or "",
    )
    missing = [
        name
        for name, value in [
            ("R2 endpoint", settings.endpoint),
            ("R2 bucket", settings.bucket),
            ("APPD_BAZEL_S3_ACCESS_KEY_ID", settings.access_key_id),
            ("APPD_BAZEL_S3_SECRET_ACCESS_KEY", settings.secret_access_key),
        ]
        if not value
    ]
    if missing:
        joined = ", ".join(missing)
        raise ValueError(f"R2 cache settings are incomplete; missing: {joined}")
    return settings


def bazel_cache_args(config: BuildCacheConfig, remote_cache_url: str | None = None) -> list[str]:
    if config.mode == "off":
        return []

    args = [
        f"--disk_cache={config.cache_dir / 'bazel-disk'}",
        f"--repository_cache={config.cache_dir / 'bazel-repository'}",
    ]
    if config.mode == "local":
        return args

    if remote_cache_url is None:
        raise ValueError(f"{config.mode} requires a bazel-remote URL")

    upload = "true" if config.mode == "r2-read-write" else "false"
    return args + [
        f"--remote_cache={remote_cache_url}",
        f"--remote_upload_local_results={upload}",
        # package_sdk() reads link-input files (.a/.lo/...) directly off
        # disk. --remote_download_outputs defaults to "toplevel", which
        # only materializes the requested target's own outputs locally --
        # intermediate outputs satisfied by a cache hit are left remote,
        # and packaging fails with a FileNotFoundError. "all" is required
        # whenever a remote cache is in play, not just for measurement.
        "--remote_download_outputs=all",
    ]


@contextlib.contextmanager
def bazel_cache(config: BuildCacheConfig | None) -> Iterator[list[str]]:
    config = config or BuildCacheConfig()
    ensure_cache_dirs(config)
    if config.mode in {"off", "local"}:
        yield bazel_cache_args(config)
        return

    port = config.bazel_remote_port or reserve_tcp_port()
    with running_bazel_remote(config, port):
        yield bazel_cache_args(config, f"http://127.0.0.1:{port}")


def ensure_cache_dirs(config: BuildCacheConfig) -> None:
    if config.mode == "off":
        return
    (config.cache_dir / "bazel-disk").mkdir(parents=True, exist_ok=True)
    (config.cache_dir / "bazel-repository").mkdir(parents=True, exist_ok=True)
    if config.mode.startswith("r2-"):
        (config.cache_dir / "bazel-remote").mkdir(parents=True, exist_ok=True)


def reserve_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def bazel_remote_s3_env(config: BuildCacheConfig) -> dict[str, str]:
    if config.r2 is None:
        raise ValueError("bazel-remote requires R2 cache settings")

    env = {
        "BAZEL_REMOTE_S3_ENDPOINT": config.r2.endpoint,
        "BAZEL_REMOTE_S3_BUCKET": config.r2.bucket,
        "BAZEL_REMOTE_S3_PREFIX": config.r2.prefix,
        "BAZEL_REMOTE_S3_AUTH_METHOD": "access_key",
        "BAZEL_REMOTE_S3_ACCESS_KEY_ID": config.r2.access_key_id,
        "BAZEL_REMOTE_S3_SECRET_ACCESS_KEY": config.r2.secret_access_key,
        "BAZEL_REMOTE_S3_BUCKET_LOOKUP_TYPE": "path",
        "BAZEL_REMOTE_S3_REGION": "auto",
        "BAZEL_REMOTE_S3_SIGNATURE_TYPE": "v4",
        "BAZEL_REMOTE_GRPC_ADDRESS": "none",
        "BAZEL_REMOTE_ACCESS_LOG_LEVEL": "none",
    }
    if config.r2.session_token:
        env["BAZEL_REMOTE_S3_SESSION_TOKEN"] = config.r2.session_token
    return env


def ensure_bazel_remote_bin(bazel_remote_bin: str | None) -> str:
    if bazel_remote_bin:
        return bazel_remote_bin

    gopath_result = subprocess.run(["go", "env", "GOPATH"], capture_output=True, text=True)
    if gopath_result.returncode != 0:
        raise RuntimeError(
            f"bazel-remote isn't installed and Go isn't available to install it: "
            f"{gopath_result.stderr.strip()}"
        )
    bin_path = Path(gopath_result.stdout.strip()) / "bin" / "bazel-remote"
    if not bin_path.exists():
        install = subprocess.run(["go", "install", BAZEL_REMOTE_MODULE], capture_output=True, text=True)
        if install.returncode != 0:
            raise RuntimeError(f"failed to install bazel-remote: {install.stderr.strip()}")
    return str(bin_path)


@contextlib.contextmanager
def running_bazel_remote(config: BuildCacheConfig, port: int) -> Iterator[None]:
    bazel_remote_bin = ensure_bazel_remote_bin(config.bazel_remote_bin)
    log_dir = config.cache_dir / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    stdout_path = log_dir / "bazel-remote.stdout.log"
    stderr_path = log_dir / "bazel-remote.stderr.log"

    env = dict(os.environ)
    env.update(bazel_remote_s3_env(config))
    env.update(
        {
            "BAZEL_REMOTE_DIR": str(config.cache_dir / "bazel-remote"),
            "BAZEL_REMOTE_MAX_SIZE": str(config.max_size_gib),
            "BAZEL_REMOTE_HTTP_ADDRESS": f"127.0.0.1:{port}",
        }
    )

    with stdout_path.open("ab") as stdout, stderr_path.open("ab") as stderr:
        process = subprocess.Popen([bazel_remote_bin], env=env, stdout=stdout, stderr=stderr)
        try:
            wait_for_bazel_remote(port, process)
            yield
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def wait_for_bazel_remote(port: int, process: subprocess.Popen[bytes], timeout_seconds: int = 30) -> None:
    url = f"http://127.0.0.1:{port}/status"
    deadline = time.monotonic() + timeout_seconds
    last_error: Exception | None = None

    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"bazel-remote exited before it became ready: {process.returncode}")
        try:
            with urllib.request.urlopen(url, timeout=1) as response:
                if response.status == 200:
                    return
        except (OSError, urllib.error.URLError) as error:
            last_error = error
        time.sleep(0.25)

    raise TimeoutError(f"timed out waiting for bazel-remote at {url}: {last_error}")


def build_sdk(
    *,
    target: str,
    output_dir: Path,
    config_path: Path = DEFAULT_UPSTREAM_CONFIG,
    source_dir: Path | None = None,
    bazel_args: list[str] | None = None,
    cache_config: BuildCacheConfig | None = None,
) -> Path:
    upstream = load_upstream_config(config_path)
    normalized_target = normalize_target(target)
    source_dir = source_dir or fetch_upstream(config_path=config_path)
    apply_overlay(source_dir)

    with bazel_cache(cache_config) as cache_args:
        command = ["bazel", "build", APPD_BAZEL_TARGET]
        command.extend(default_bazel_args(normalized_target))
        command.extend(cache_args)
        command.extend(bazel_args or [])
        subprocess.run(command, cwd=source_dir, check=True)

    params_path = find_link_params(source_dir)
    header_path = source_dir / "src" / "workerd" / "server" / "appd_workerd.h"
    return package_sdk(
        params_path=params_path,
        output_dir=output_dir,
        target=normalized_target,
        upstream_tag=upstream["tag"],
        upstream_commit=upstream["commit"],
        header_path=header_path,
    )


def find_link_params(source_dir: Path) -> Path:
    bazel_bin = source_dir / "bazel-bin"
    matches = sorted(bazel_bin.rglob("*appd-workerd-link-anchor*.params"))
    if not matches:
        raise FileNotFoundError(f"no appd workerd link params found under {bazel_bin}")
    if len(matches) > 1:
        names = "\n".join(str(path) for path in matches)
        raise ValueError(f"multiple appd workerd link params found:\n{names}")
    return matches[0]
