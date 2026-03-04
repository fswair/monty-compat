"""Tests for monty_compat — no local Monty checkout required (uses disk cache)."""

from monty_compat import MontyCapabilities


# ── Class-level accessor methods (disk cache) ─────────────────────────


def test_get_modules() -> None:
    mods = MontyCapabilities.get_modules()
    assert isinstance(mods, frozenset)
    for expected in ("re", "os", "sys", "asyncio", "pathlib", "typing"):
        assert expected in mods, f"'{expected}' missing from get_modules()"


def test_get_builtins() -> None:
    builtins = MontyCapabilities.get_builtins()
    assert isinstance(builtins, frozenset)
    for expected in ("abs", "len", "print", "sum", "sorted"):
        assert expected in builtins, f"'{expected}' missing from get_builtins()"


def test_get_types() -> None:
    types = MontyCapabilities.get_types()
    assert isinstance(types, frozenset)
    for expected in ("int", "str", "list", "dict", "bool", "set", "tuple"):
        assert expected in types, f"'{expected}' missing from get_types()"


def test_get_exception_types() -> None:
    excs = MontyCapabilities.get_exception_types()
    assert isinstance(excs, frozenset)
    for expected in ("ValueError", "TypeError", "RuntimeError", "KeyError"):
        assert expected in excs, f"'{expected}' missing from get_exception_types()"


def test_get_attrs_of_module_asyncio() -> None:
    attrs = MontyCapabilities.get_attrs_of_module("asyncio")
    assert "gather" in attrs
    assert "run" in attrs


def test_get_attrs_of_module_re() -> None:
    attrs = MontyCapabilities.get_attrs_of_module("re")
    for fn in ("search", "match", "sub", "findall", "compile", "fullmatch", "split"):
        assert fn in attrs, f"'{fn}' missing from re module attrs"


def test_get_attrs_of_module_unknown_returns_empty() -> None:
    assert MontyCapabilities.get_attrs_of_module("math") == frozenset()
    assert MontyCapabilities.get_attrs_of_module("nonexistent") == frozenset()
