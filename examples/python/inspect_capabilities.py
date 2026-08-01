"""Inspect modules and nested runtime-type attributes in Monty's Rust source."""

from __future__ import annotations

import argparse
from pathlib import Path

from monty_compat import MontyCapabilities


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--monty-root",
        type=Path,
        help="exact local Monty checkout; latest released source is downloaded when omitted",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    capabilities = (
        MontyCapabilities.from_local(args.monty_root)
        if args.monty_root is not None
        else MontyCapabilities.from_github()
    )

    print(f"modules: {len(capabilities.modules)}")
    print(f"runtime types: {len(capabilities.type_attributes)}")
    print("pathlib exports:", sorted(capabilities.get_attributes("pathlib")))
    print("Path attributes:", sorted(capabilities.get_attributes("pathlib.Path")))
    print("Path.is_dir:", capabilities.supports_path("pathlib.Path.is_dir"))


if __name__ == "__main__":
    main()
