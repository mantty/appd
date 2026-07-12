#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

import appd_bare


def main() -> None:
    parser = argparse.ArgumentParser(description="Build an appd Bare SDK")
    parser.add_argument("--target", choices=sorted(appd_bare.TARGETS), required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--target-root", type=Path, default=appd_bare.DEFAULT_TARGET_ROOT)
    args = parser.parse_args()
    output = args.output or args.target_root / "sdk" / args.target
    appd_bare.build_sdk(args.target, output, args.target_root)


if __name__ == "__main__":
    main()
