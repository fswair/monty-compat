#!/usr/bin/env python3
"""Transpile one source input and emit a deterministic JSON result."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from monty_compat import TranspilationError, transpiler


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Transpile a Python snippet for an exact bundled Monty release."
    )
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--code", help="inline Python module source")
    source.add_argument("--file", type=Path, help="UTF-8 Python source path")
    parser.add_argument(
        "--release",
        default="verified",
        help="verified, latest, 0.0.19, or v0.0.19",
    )
    parser.add_argument("--output", type=Path, help="optional lowered-source output path")
    return parser.parse_args()


def read_source(args: argparse.Namespace) -> str:
    if args.code is not None:
        return args.code
    if args.file is not None:
        return args.file.read_text(encoding="utf-8")
    return sys.stdin.read()


def emit(payload: dict[str, object]) -> None:
    print(json.dumps(payload, ensure_ascii=False, sort_keys=True))


def main() -> int:
    args = parse_args()
    try:
        source = read_source(args)
        lowered = transpiler(source, release=args.release)
    except (OSError, UnicodeError, TranspilationError) as error:
        emit(
            {
                "error": str(error),
                "error_type": type(error).__name__,
                "ok": False,
                "release": args.release,
            }
        )
        return 2

    if args.output is not None:
        try:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(lowered, encoding="utf-8")
        except (OSError, UnicodeError) as error:
            emit(
                {
                    "error": str(error),
                    "error_type": type(error).__name__,
                    "ok": False,
                    "release": args.release,
                }
            )
            return 2

    emit(
        {
            "changed": lowered != source,
            "lowered": lowered,
            "ok": True,
            "output": str(args.output) if args.output is not None else None,
            "release": args.release,
            "source_bytes": len(source.encode("utf-8")),
        }
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
