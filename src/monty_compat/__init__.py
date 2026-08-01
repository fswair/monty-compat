"""Release-aware Python compatibility for the Monty interpreter.

Use :func:`transpiler` to produce ordinary Python source for an exact bundled
Monty release. It lowers only evidence-backed seams whose semantics can be
preserved and raises :class:`TranspilationError` otherwise::

    from monty_compat import transpiler

    lowered = transpiler(source, release="0.0.19")

Use :class:`MontyCapabilities` when the extracted capability graph itself is
needed::

    caps = MontyCapabilities.from_local("/path/to/monty")
    caps.supports_path("pathlib.Path.is_dir")

Static extraction may use a 12-hour disk cache. The default verified
transpilation hot path is native Rust, embeds versioned manifests, performs no
network access or probes, and never executes the supplied source. The explicit
``latest`` mode resolves a bounded and validated remote manifest once per
process before lowering.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from ._native import TranspilationError, transpiler
from .capabilities import MontyCapabilities
from .generated_discovery import GeneratedProbeConfig

__all__ = [
    "GeneratedProbeConfig",
    "MontyCapabilities",
    "TranspilationError",
    "discover_latest_release",
    "transpiler",
    "write_manifest",
]


def discover_latest_release(
    *,
    allow_version_mismatch: bool = False,
    generated_config: GeneratedProbeConfig | None = None,
) -> dict[str, Any]:
    """Build the latest release's static and behavioral capability manifest."""
    from .discovery import discover_latest_release as _discover

    return _discover(
        allow_version_mismatch=allow_version_mismatch,
        generated_config=generated_config,
    )


def write_manifest(manifest: dict[str, Any], output: str | Path) -> Path:
    """Write a capability manifest to disk."""
    from .discovery import write_manifest as _write

    return _write(manifest, output)
