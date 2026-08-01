from __future__ import annotations

import ast
import inspect

import pytest

import monty_compat
from monty_compat import TranspilationError, transpiler

MATCH_SOURCE = """\
value = 2
match value:
    case 1:
        result = "one"
    case _:
        result = "other"
result
"""


def test_public_function_lowers_without_exposing_a_transpiler_class() -> None:
    lowered = transpiler(MATCH_SOURCE)

    ast.parse(lowered)
    assert "match value:" not in lowered


def test_misleading_static_source_checker_is_not_public() -> None:
    assert "monty_compat" not in monty_compat.__all__
    assert not hasattr(monty_compat, "monty_compat")
    assert not hasattr(monty_compat.MontyCapabilities, "check_code")


def test_release_aliases_produce_the_same_source() -> None:
    verified = transpiler(MATCH_SOURCE)

    assert inspect.signature(transpiler).parameters["release"].default == "verified"
    assert transpiler(MATCH_SOURCE, "verified") == verified
    assert transpiler(MATCH_SOURCE, "0.0.19") == verified
    assert transpiler(MATCH_SOURCE, "v0.0.19") == verified


def test_supported_source_is_returned_unchanged() -> None:
    assert transpiler("value = 1\nvalue\n") == "value = 1\nvalue\n"


def test_unknown_release_does_not_fall_back() -> None:
    with pytest.raises(TranspilationError, match="not bundled"):
        transpiler("1 + 1", "0.0.20")


def test_non_representable_source_is_rejected() -> None:
    with pytest.raises(TranspilationError, match="cannot be preserved"):
        transpiler("def values():\n    yield 1\n")


def test_lowered_source_runs_on_exact_monty_release() -> None:
    pydantic_monty = pytest.importorskip("pydantic_monty")
    assert pydantic_monty.__version__ == "0.0.19"

    with pydantic_monty.Monty() as pool:
        with pool.checkout(assert_message_annotations=False) as session:
            assert session.feed_run(transpiler(MATCH_SOURCE)) == "other"
