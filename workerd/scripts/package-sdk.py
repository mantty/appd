#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

import appd_workerd


def main() -> None:
    parser = argparse.ArgumentParser(description="Package a static appd workerd SDK.")
    parser.add_argument("--params", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--upstream-tag", required=True)
    parser.add_argument("--upstream-commit", required=True)
    parser.add_argument("--header", type=Path, required=True)
    args = parser.parse_args()

    manifest = appd_workerd.package_sdk(
        params_path=args.params,
        output_dir=args.output,
        target=args.target,
        upstream_tag=args.upstream_tag,
        upstream_commit=args.upstream_commit,
        header_path=args.header,
    )
    print(manifest)


if __name__ == "__main__":
    main()
