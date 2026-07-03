#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

import appd_workerd


def main() -> None:
    parser = argparse.ArgumentParser(description="Build and package the appd workerd SDK.")
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--config", type=Path, default=appd_workerd.DEFAULT_UPSTREAM_CONFIG)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--cache", choices=appd_workerd.CACHE_MODES)
    parser.add_argument("--cache-dir", type=Path)
    parser.add_argument("--bazel-remote-bin")
    parser.add_argument("--bazel-remote-port", type=int)
    parser.add_argument("--bazel-remote-max-size-gib", type=int)
    parser.add_argument("--r2-endpoint")
    parser.add_argument("--r2-account-id")
    parser.add_argument("--r2-bucket")
    parser.add_argument("--r2-prefix")
    parser.add_argument("--r2-access-key-id")
    parser.add_argument("--r2-secret-access-key")
    parser.add_argument("--r2-session-token")
    parser.add_argument("bazel_args", nargs=argparse.REMAINDER)
    args = parser.parse_args()

    bazel_args = args.bazel_args
    if bazel_args and bazel_args[0] == "--":
        bazel_args = bazel_args[1:]

    normalized_target = appd_workerd.normalize_target(args.target)
    output = args.output or appd_workerd.DEFAULT_TARGET_ROOT / "sdk" / normalized_target
    manifest = appd_workerd.build_sdk(
        target=normalized_target,
        output_dir=output,
        config_path=args.config,
        source_dir=args.source,
        bazel_args=bazel_args,
        cache_config=appd_workerd.cache_config_from_env(
            mode=args.cache,
            cache_dir=args.cache_dir,
            bazel_remote_bin=args.bazel_remote_bin,
            bazel_remote_port=args.bazel_remote_port,
            max_size_gib=args.bazel_remote_max_size_gib,
            r2_endpoint=args.r2_endpoint,
            r2_account_id=args.r2_account_id,
            r2_bucket=args.r2_bucket,
            r2_prefix=args.r2_prefix,
            r2_access_key_id=args.r2_access_key_id,
            r2_secret_access_key=args.r2_secret_access_key,
            r2_session_token=args.r2_session_token,
        ),
    )
    print(manifest)


if __name__ == "__main__":
    main()
