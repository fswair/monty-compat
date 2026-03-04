"""monty_compat — Detect Monty-supported Python features from source.

Downloads the Monty Rust source from GitHub, parses the builtin function,
type constructor, exception type, and module enums, then exposes an AST-based
compatibility checker plus the raw capability sets.

Quick start::

    # One-shot check — loads (or builds) capabilities automatically
    from monty_compat import monty_compat

    ok, reasons = monty_compat("import re\\nx = re.sub('a', 'b', 'abc')")
    # ok=False, reasons=["module 're' is not supported by Monty"]

    ok, reasons = monty_compat(code, cache='regenerate')  # force rebuild
    ok, reasons = monty_compat(code, cache='off')         # skip cache

Cache files live at ``~/.monty_compat/monty_{version}_compat.json`` and
expire after 12 hours by default.

Lower-level API::

    from monty_compat import MontyCapabilities

    caps = MontyCapabilities.from_local('/path/to/monty')
    caps = MontyCapabilities.from_github()

    caps.builtin_functions   # frozenset — abs, all, any, …
    caps.modules             # frozenset — sys, typing, asyncio, pathlib, os
    caps.module_attributes   # dict     — {'asyncio': {'gather','run'}, …}
    caps.type_constructors   # frozenset — int, str, list, …
    caps.exception_types     # frozenset — ValueError, TypeError, …

    ok, reasons = caps.check_code(some_code)
"""

from __future__ import annotations

from pathlib import Path
from typing import Literal

from .cache import _DEFAULT_TTL, get_capabilities
from .capabilities import MontyCapabilities

__all__ = ["MontyCapabilities", "monty_compat"]


def monty_compat(
    code: str,
    *,
    cache: Literal["auto", "regenerate", "off"] = "auto",
    ttl: int = _DEFAULT_TTL,
    cache_dir: str | Path | None = None,
    monty_root: str | Path | None = None,
) -> tuple[bool, list[str]]:
    """Check whether *code* can run in the Monty sandbox.

    Capabilities are loaded from the on-disk cache (``~/.monty_compat/``) when
    available and fresh, otherwise built by parsing the Monty Rust source.

    Args:
        code: Python source code to analyse.
        cache: Cache strategy.
            ``'auto'`` (default) — use cache if fresh, rebuild on expiry;
            ``'regenerate'`` — always rebuild and overwrite cache;
            ``'off'`` — skip cache entirely.
        ttl: Cache time-to-live in seconds (default: 43 200 = 12 h).
        cache_dir: Override the default ``~/.monty_compat/`` directory.
        monty_root: Path to a local Monty repo checkout.  When given, source
            is read from disk instead of being downloaded from GitHub.

    Returns:
        ``(can_run, reasons)`` where *reasons* is an empty list when Monty
        should be able to execute the code without errors.

    Example::

        ok, reasons = monty_compat("x = [i*2 for i in range(10)]")
        # ok=True, reasons=[]

        ok, reasons = monty_compat("import json; json.loads('{}')")
        # ok=False, reasons=["module 'json' is not supported by Monty"]
    """
    caps = get_capabilities(
        cache=cache,
        ttl=ttl,
        cache_dir=cache_dir,
        monty_root=monty_root,
    )
    return caps.check_code(code)
