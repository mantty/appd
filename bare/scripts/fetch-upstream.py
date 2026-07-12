#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

import appd_bare


def main() -> None:
    parser = argparse.ArgumentParser(description="Fetch pinned BareKit source")
    parser.add_argument("--target-root", type=Path, default=appd_bare.DEFAULT_TARGET_ROOT)
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    print(appd_bare.fetch_upstream(args.target_root, args.force))


if __name__ == "__main__":
    main()
