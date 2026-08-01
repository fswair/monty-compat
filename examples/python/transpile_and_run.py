"""Transpile Python for exact Monty 0.0.19 and execute it with Monty's API."""

from __future__ import annotations

from pydantic_monty import Monty

from monty_compat import transpiler

SOURCE = """\
value = 2
match value:
    case 2:
        result = f"ok:{value}"
    case _:
        result = "other"
result
"""


def main() -> None:
    lowered = transpiler(SOURCE, release="0.0.19")
    print("Lowered source:\n")
    print(lowered)

    with Monty() as pool:
        with pool.checkout() as session:
            result = session.feed_run(lowered)
    print(f"Monty result: {result!r}")
    assert result == "ok:2"


if __name__ == "__main__":
    main()
