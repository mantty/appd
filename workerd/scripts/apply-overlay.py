#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

import appd_workerd


def main() -> None:
    parser = argparse.ArgumentParser(description="Apply appd workerd overlay to upstream source.")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--overlay", type=Path, default=appd_workerd.DEFAULT_OVERLAY_ROOT)
    parser.add_argument("--patches", type=Path, default=appd_workerd.DEFAULT_PATCHES_ROOT)
    args = parser.parse_args()

    appd_workerd.apply_overlay(args.source, args.overlay, args.patches)


if __name__ == "__main__":
    main()
