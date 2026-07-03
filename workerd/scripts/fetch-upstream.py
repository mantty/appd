#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

import appd_workerd


def main() -> None:
    parser = argparse.ArgumentParser(description="Fetch pinned upstream workerd source.")
    parser.add_argument("--config", type=Path, default=appd_workerd.DEFAULT_UPSTREAM_CONFIG)
    parser.add_argument("--target-root", type=Path, default=appd_workerd.DEFAULT_TARGET_ROOT)
    parser.add_argument("--force", action="store_true")
    parser.add_argument("--refresh", action="store_true")
    args = parser.parse_args()

    source_dir = appd_workerd.fetch_upstream(
        config_path=args.config,
        target_root=args.target_root,
        force=args.force,
        refresh=args.refresh,
    )
    print(source_dir)


if __name__ == "__main__":
    main()
