"""Handle release and semantic failures without bypassing the transpiler."""

from __future__ import annotations

from monty_compat import TranspilationError, transpiler


def show_failure(label: str, source: str, release: str = "verified") -> None:
    try:
        transpiler(source, release=release)
    except TranspilationError as error:
        print(f"{label}: {error}")
    else:
        raise AssertionError(f"{label} unexpectedly transpiled")


def main() -> None:
    show_failure("unknown release", "1 + 1\n", release="0.0.20")
    show_failure("generator semantics", "def values():\n    yield 1\n")


if __name__ == "__main__":
    main()
