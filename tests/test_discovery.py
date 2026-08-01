"""Tests for behavioral capability discovery and manifest assembly."""

from __future__ import annotations

from typing import Any

from monty_compat import MontyCapabilities
from monty_compat.discovery import ReleaseFingerprint, build_manifest
from monty_compat.probe_catalog import BASELINE_PROBES
from monty_compat.probe_catalog_protocols import PROTOCOL_MATRIX_PROBES
from monty_compat.probes import (
    ProbeSpec,
    ProbeStatus,
    iter_invalid_catalog_entries,
    run_on_cpython,
    run_probe,
)


class _ValueRunner:
    def __init__(self, value: Any) -> None:
        self.value = value

    def run(self, source: str) -> Any:
        del source
        return self.value


class _ErrorRunner:
    def __init__(self, error: BaseException) -> None:
        self.error = error

    def run(self, source: str) -> Any:
        del source
        raise self.error


def _probe() -> ProbeSpec:
    return ProbeSpec("expression.add", "expression", "1 + 2", "Integer addition")


def test_baseline_catalog_is_unique_valid_and_cpython_executable() -> None:
    assert len(BASELINE_PROBES) == 269
    assert len(PROTOCOL_MATRIX_PROBES) == 33
    assert list(iter_invalid_catalog_entries(BASELINE_PROBES)) == []
    assert all(run_on_cpython(spec.source) is not NotImplemented for spec in BASELINE_PROBES)


def test_probe_classifies_support_mismatch_and_parser_rejection() -> None:
    supported = run_probe(_probe(), _ValueRunner(3))
    mismatched = run_probe(_probe(), _ValueRunner(True))
    parser_error_type = type("MontyRuntimeError", (RuntimeError,), {})
    rejected = run_probe(
        _probe(),
        _ErrorRunner(parser_error_type("syntax parser does not yet support this expression")),
    )

    assert supported.status is ProbeStatus.SUPPORTED
    assert mismatched.status is ProbeStatus.SEMANTIC_MISMATCH
    assert rejected.status is ProbeStatus.UNSUPPORTED_PARSE


def test_manifest_combines_static_and_behavioral_capabilities() -> None:
    release = ReleaseFingerprint(
        repository="pydantic/monty",
        tag="v1.2.3",
        published_at="2026-01-01T00:00:00Z",
        release_url="https://example.test/release",
    )
    capabilities = MontyCapabilities(
        builtin_functions=frozenset({"len"}),
        modules=frozenset({"math"}),
        module_attributes={"math": frozenset({"sqrt"})},
    )
    result = run_probe(_probe(), _ValueRunner(3))

    manifest = build_manifest(release, capabilities, [result], runtime_version="1.2.3")

    assert manifest["target"]["tag"] == "v1.2.3"
    assert manifest["static_capabilities"]["module_attributes"] == {"math": ["sqrt"]}
    behavioral = manifest["behavioral_capabilities"]
    assert behavioral["summary"]["supported"] == 1
    assert behavioral["features"]["expression.add"]["status"] == "supported"
