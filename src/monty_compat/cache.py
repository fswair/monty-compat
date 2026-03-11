"""Persistent cache for MontyCapabilities with TTL and version-based paths.

Cache files live at ``~/.monty_compat/monty_{version}_compat.json``.  The
default TTL is 12 hours; expired caches trigger a full rebuild from GitHub or
from a local checkout, after which the new data is saved back to disk.
"""

from __future__ import annotations

import json
import time
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .capabilities import MontyCapabilities

_DEFAULT_TTL = 12 * 3600  # 12 hours in seconds
_DEFAULT_CACHE_DIR = Path.home() / ".monty_compat"
_CACHE_VERSION_KEY = "cache_schema_version"
_CACHE_SCHEMA = 1  # bump when the JSON layout changes incompatibly


# ── Version detection ─────────────────────────────────────────────────


def _installed_monty_version() -> str:
    """Return the installed pydantic-monty version string, or 'unknown'."""
    try:
        from importlib.metadata import version as _pkg_version

        return _pkg_version("pydantic-monty")
    except Exception:
        return "unknown"


# ── Path helpers ──────────────────────────────────────────────────────


def cache_path(
    version: str | None = None,
    cache_dir: str | Path | None = None,
) -> Path:
    """Return the absolute path for the cache file of *version*.

    If *version* is ``None``, the installed ``pydantic-monty`` version is used.
    If *cache_dir* is ``None``, ``~/.monty_compat/`` is used.
    """
    if version is None:
        version = _installed_monty_version()
    base = Path(cache_dir) if cache_dir else _DEFAULT_CACHE_DIR
    # Sanitise version string so it is safe to embed in a filename
    safe_ver = version.replace("+", "_").replace("/", "_")
    return base / f"monty_{safe_ver}_compat.json"


# ── Load ──────────────────────────────────────────────────────────────


def load_cache(
    version: str | None = None,
    *,
    ttl: int = _DEFAULT_TTL,
    cache_dir: str | Path | None = None,
) -> MontyCapabilities | None:
    """Load capabilities from the on-disk cache if it exists and is fresh.

    Returns ``None`` when:
    - The cache file does not exist.
    - The cache is older than *ttl* seconds.
    - The cache was written by an incompatible schema version.

    Args:
        version: pydantic-monty version string.  Defaults to the installed one.
        ttl: Maximum cache age in seconds (default: 12 hours).
        cache_dir: Directory that holds cache files.
    """
    from .capabilities import MontyCapabilities

    path = cache_path(version, cache_dir)
    if not path.exists():
        return None

    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return None

    # Schema compatibility guard
    if raw.get(_CACHE_VERSION_KEY, 0) != _CACHE_SCHEMA:
        return None

    # TTL check
    created_at: float = raw.get("created_at", 0.0)
    if ttl > 0 and (time.time() - created_at) > ttl:
        return None

    try:
        return MontyCapabilities.from_dict(raw["capabilities"])
    except (KeyError, TypeError):
        return None


# ── Save ─────────────────────────────────────────────────────────────


def save_cache(
    caps: MontyCapabilities,
    version: str | None = None,
    *,
    cache_dir: str | Path | None = None,
) -> Path:
    """Persist *caps* to the version-keyed cache file.

    Creates ``~/.monty_compat/`` (or *cache_dir*) if it does not exist.

    Returns the path of the written file.
    """
    path = cache_path(version, cache_dir)
    path.parent.mkdir(parents=True, exist_ok=True)

    payload = {
        _CACHE_VERSION_KEY: _CACHE_SCHEMA,
        "monty_version": version or _installed_monty_version(),
        "created_at": time.time(),
        "ttl_hint": _DEFAULT_TTL,
        "capabilities": caps.to_dict(),
    }
    path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    return path


# ── Convenience: build + cache ────────────────────────────────────────


def get_capabilities(
    *,
    cache: str = "auto",
    ttl: int = _DEFAULT_TTL,
    cache_dir: str | Path | None = None,
    monty_root: str | Path | None = None,
    version: str | None = None,
    only_released: bool = True,
) -> MontyCapabilities:
    """Return a :class:`~monty_compat.MontyCapabilities` instance, using the
    on-disk cache when available and valid.

    Args:
        cache: Cache strategy —
            ``'auto'``       load from cache if fresh, otherwise build and save;
            ``'regenerate'`` always rebuild and overwrite the cache;
            ``'off'``        skip the cache entirely (no read, no write).
        ttl: Cache time-to-live in seconds (default: 43 200 = 12 h).
        cache_dir: Override the default ``~/.monty_compat/`` directory.
        monty_root: Path to a local Monty repo checkout.  When given, source
            code is read from disk instead of downloaded from GitHub.
        version: Explicit version string for the cache key.  If ``None`` the
            key is derived from *only_released* (see below).
        only_released: When ``True`` (default), parse capabilities from the
            latest tagged release instead of the ``main`` branch.  This avoids
            false compatibility signals for unreleased changes.  The cache key
            is ``'latest-release'`` in this mode and ``'main'`` otherwise.
    """
    from .capabilities import MontyCapabilities

    if cache not in ("auto", "regenerate", "off"):
        raise ValueError(f"cache must be 'auto', 'regenerate', or 'off'; got {cache!r}")

    if version is not None:
        ver = version
    elif monty_root is not None:
        ver = _installed_monty_version()
    elif only_released:
        ver = "latest-release"
    else:
        ver = "main"

    # ── Try cache (unless disabled or forced regenerate) ────────────
    if cache == "auto":
        cached = load_cache(ver, ttl=ttl, cache_dir=cache_dir)
        if cached is not None:
            return cached

    # ── Build from source ────────────────────────────────────────────
    if monty_root is not None:
        caps = MontyCapabilities.from_local(monty_root)
    else:
        caps = MontyCapabilities.from_github(only_released=only_released)

    # ── Persist ─────────────────────────────────────────────────────
    if cache != "off":
        save_cache(caps, ver, cache_dir=cache_dir)

    return caps
