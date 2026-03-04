"""Basic tests for monty_compat using the local Monty checkout."""

from pathlib import Path

import pytest

from monty_compat import MontyCapabilities, monty_compat

MONTY_ROOT = Path(__file__).parent.parent.parent / "monty"
SKIP_IF_NO_LOCAL = pytest.mark.skipif(
    not MONTY_ROOT.exists(),
    reason="Local monty checkout not found",
)


@pytest.fixture(scope="module")
def caps() -> MontyCapabilities:
    return MontyCapabilities.from_local(MONTY_ROOT)


# ── check_code: pure Python ───────────────────────────────────────────


@SKIP_IF_NO_LOCAL
def test_pure_code_passes(caps: MontyCapabilities) -> None:
    code = "def factorial(n):\n    return 1 if n <= 1 else n * factorial(n-1)"
    ok, reasons = caps.check_code(code)
    assert ok
    assert reasons == []


@SKIP_IF_NO_LOCAL
def test_fizzbuzz_passes(caps: MontyCapabilities) -> None:
    code = (
        "result = []\n"
        "for i in range(1, 101):\n"
        '    if i % 15 == 0: result.append("FizzBuzz")\n'
        '    elif i % 3 == 0: result.append("Fizz")\n'
        '    elif i % 5 == 0: result.append("Buzz")\n'
        "    else: result.append(str(i))\n"
    )
    ok, _ = caps.check_code(code)
    assert ok


# ── check_code: unsupported imports ──────────────────────────────────


@SKIP_IF_NO_LOCAL
def test_json_import_fails(caps: MontyCapabilities) -> None:
    ok, reasons = caps.check_code("import json\njson.loads('{}')")
    assert not ok
    assert any("json" in r for r in reasons)


@SKIP_IF_NO_LOCAL
def test_collections_import_fails(caps: MontyCapabilities) -> None:
    ok, reasons = caps.check_code("from collections import Counter")
    assert not ok


# ── check_code: supported modules ────────────────────────────────────


@SKIP_IF_NO_LOCAL
def test_re_import_passes(caps: MontyCapabilities) -> None:
    ok, _ = caps.check_code("import re\nre.search('[0-9]+', '123')")
    assert ok


@SKIP_IF_NO_LOCAL
def test_typing_optional_passes(caps: MontyCapabilities) -> None:
    ok, _ = caps.check_code("from typing import Optional\nx: Optional[int] = None")
    assert ok


@SKIP_IF_NO_LOCAL
def test_from_asyncio_unknown_fails(caps: MontyCapabilities) -> None:
    ok, reasons = caps.check_code("from asyncio import subprocess")
    assert not ok
    assert any("subprocess" in r for r in reasons)


# ── module_attributes populated ──────────────────────────────────────


@SKIP_IF_NO_LOCAL
def test_module_attributes_present(caps: MontyCapabilities) -> None:
    assert "asyncio" in caps.module_attributes
    assert "gather" in caps.module_attributes["asyncio"]
    assert "os" in caps.module_attributes
    assert "getenv" in caps.module_attributes["os"]
    assert "pathlib" in caps.module_attributes
    assert "Path" in caps.module_attributes["pathlib"]


# ── default monty_compat() function ──────────────────────────────────


def test_default_function_cache_off() -> None:
    if not MONTY_ROOT.exists():
        pytest.skip("Local monty checkout not found")
    ok, _ = monty_compat("x = sum(range(10))", cache="off", monty_root=MONTY_ROOT)
    assert ok


def test_default_function_unsupported_cache_off() -> None:
    if not MONTY_ROOT.exists():
        pytest.skip("Local monty checkout not found")
    ok, reasons = monty_compat("import math", cache="off", monty_root=MONTY_ROOT)
    assert not ok
    assert any("math" in r for r in reasons)


# ── JSON round-trip ───────────────────────────────────────────────────


@SKIP_IF_NO_LOCAL
def test_roundtrip(caps: MontyCapabilities) -> None:
    restored = MontyCapabilities.from_dict(caps.to_dict())
    assert restored.builtin_functions == caps.builtin_functions
    assert restored.modules == caps.modules
    assert restored.module_attributes == caps.module_attributes
