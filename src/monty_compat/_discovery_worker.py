"""Private JSONL worker for CPython oracle and pysource-codegen operations."""

from __future__ import annotations

import contextlib
import hashlib
import io
import json
import math
import platform
import sys
from dataclasses import asdict
from datetime import datetime, timezone
from importlib.metadata import PackageNotFoundError, version
from typing import Any

from .generated_discovery import (
    GeneratedProbeConfig,
    _GeneratedGuardError,
    _GeneratedSourceError,
    _load_generator,
    prepare_inert_source,
)
from .probe_catalog import BASELINE_PROBES
from .probes import _json_safe, ast_nodes_in, run_on_cpython

_generator: Any = None
_generator_version: str | None = None
_minimizer: Any = None
_minimizer_version: str | None = None

_BIGINT_WIRE_KEY = "__monty_compat_bigint__"
_NONFINITE_WIRE_KEY = "__monty_compat_nonfinite__"
_MAX_PROTOCOL_BYTES = 2 * 1024 * 1024


class _MinimizationBudgetExceeded(RuntimeError):
    """The configured number of candidate checks was exhausted."""


def _wire_json_safe(value: Any) -> Any:
    """Encode values that JSON cannot round-trip portably over the worker pipe."""
    if isinstance(value, bool) or value is None or isinstance(value, str):
        return value
    if isinstance(value, int):
        if -(2**63) <= value <= 2**64 - 1:
            return value
        return {_BIGINT_WIRE_KEY: str(value)}
    if isinstance(value, float):
        if math.isnan(value):
            return {_NONFINITE_WIRE_KEY: "nan"}
        if math.isinf(value):
            return {_NONFINITE_WIRE_KEY: "inf" if value > 0 else "-inf"}
        return value
    if isinstance(value, list):
        return [_wire_json_safe(item) for item in value]
    if isinstance(value, dict):
        return {key: _wire_json_safe(item) for key, item in value.items()}
    raise TypeError(f"oracle value is not JSON-safe: {type(value).__name__}")


def _generator_identity() -> str | None:
    global _generator, _generator_version
    if _generator is None:
        _generator, _generator_version = _load_generator()
    return _generator_version


def _minimizer_identity() -> str | None:
    global _minimizer, _minimizer_version
    if _minimizer is None:
        try:
            import pysource_minimize
        except ImportError as exc:
            raise RuntimeError(
                "generated minimization requires 'pysource-minimize'; "
                "install 'monty-compat[discovery]'"
            ) from exc
        _minimizer = pysource_minimize.minimize
        candidate_version = getattr(pysource_minimize, "version", None)
        if isinstance(candidate_version, str):
            _minimizer_version = candidate_version
        else:
            try:
                _minimizer_version = version("pysource-minimize")
            except PackageNotFoundError:
                _minimizer_version = None
    return _minimizer_version


def _generate(request: dict[str, Any]) -> dict[str, Any]:
    _generator_identity()
    config = GeneratedProbeConfig(**request["config"])
    seed = request["seed"]
    if not isinstance(seed, int):
        raise TypeError("seed must be an integer")
    try:
        source = _generator(
            seed,
            node_limit=config.node_limit,
            depth_limit=config.depth_limit,
            root_node="Module",
        )
        if not isinstance(source, str):
            raise TypeError("generator returned a non-string source")
    except BaseException as exc:
        return {
            "kind": "generation_error",
            "seed": seed,
            "error_type": type(exc).__name__,
            "error_message": str(exc),
        }

    source_sha256 = hashlib.sha256(source.encode()).hexdigest()
    try:
        inert_source, ast_nodes, ast_node_count = prepare_inert_source(source, config)
    except _GeneratedSourceError as exc:
        return {
            "kind": "generation_error",
            "seed": seed,
            "source": source,
            "source_sha256": source_sha256,
            "error_type": type(exc).__name__,
            "error_message": str(exc),
        }
    except _GeneratedGuardError as exc:
        return {
            "kind": "guard_rejected",
            "seed": seed,
            "source": source,
            "source_sha256": source_sha256,
            "error_type": type(exc).__name__,
            "error_message": str(exc),
        }
    return {
        "kind": "prepared",
        "seed": seed,
        "source": source,
        "source_sha256": source_sha256,
        "inert_source": inert_source,
        "ast_nodes": ast_nodes,
        "ast_node_count": ast_node_count,
    }


def _oracle(request: dict[str, Any]) -> dict[str, Any]:
    source = request["source"]
    if not isinstance(source, str):
        raise TypeError("source must be a string")
    try:
        return {
            "kind": "return",
            "value": _wire_json_safe(_json_safe(run_on_cpython(source))),
            "ast_nodes": ast_nodes_in(source),
        }
    except BaseException as exc:
        return {
            "kind": "raise",
            "error_type": type(exc).__name__,
            "error_message": str(exc),
            "ast_nodes": (),
        }


def _minimize(
    request: dict[str, Any], protocol_in: Any, protocol_out: Any
) -> dict[str, Any]:
    _minimizer_identity()
    source = request.get("source")
    max_checks = request.get("max_checks")
    if not isinstance(source, str):
        raise TypeError("source must be a string")
    if not isinstance(max_checks, int) or isinstance(max_checks, bool) or max_checks <= 0:
        raise TypeError("max_checks must be a positive integer")
    config = GeneratedProbeConfig(**request["config"])
    checks = 0

    def checker(candidate: str) -> bool:
        nonlocal checks
        if checks >= max_checks:
            raise _MinimizationBudgetExceeded(
                f"minimizer exceeded the {max_checks}-candidate limit"
            )
        checks += 1
        try:
            inert_source, _, _ = prepare_inert_source(candidate, config)
        except (_GeneratedSourceError, _GeneratedGuardError):
            return False
        event = {
            "event": "minimize_candidate",
            "candidate_id": checks,
            "inert_source": inert_source,
        }
        protocol_out.write(json.dumps(event, ensure_ascii=False, sort_keys=True) + "\n")
        protocol_out.flush()
        verdict_line = protocol_in.readline(_MAX_PROTOCOL_BYTES + 1)
        if not verdict_line:
            raise RuntimeError("minimizer verdict stream closed")
        if len(verdict_line.encode()) > _MAX_PROTOCOL_BYTES or not verdict_line.endswith("\n"):
            raise RuntimeError("minimizer verdict exceeds the protocol limit")
        verdict = json.loads(verdict_line)
        if not isinstance(verdict, dict) or verdict.get("op") != "minimize_verdict":
            raise RuntimeError("invalid minimizer verdict operation")
        if verdict.get("candidate_id") != checks:
            raise RuntimeError("minimizer verdict candidate id does not match")
        preserves = verdict.get("preserves")
        if not isinstance(preserves, bool):
            raise RuntimeError("minimizer verdict must contain a boolean 'preserves'")
        return preserves

    try:
        minimized = _minimizer(source, checker, retries=0, compilable=True)
        if not isinstance(minimized, str):
            raise TypeError("minimizer returned a non-string source")
        _, ast_nodes, ast_node_count = prepare_inert_source(minimized, config)
    except BaseException as exc:
        return {
            "kind": "minimization_error",
            "checks": checks,
            "error_type": type(exc).__name__,
            "error_message": str(exc),
        }
    if len(minimized.encode()) >= len(source.encode()):
        return {"kind": "unchanged", "checks": checks}
    return {
        "kind": "minimized",
        "source": minimized,
        "source_sha256": hashlib.sha256(minimized.encode()).hexdigest(),
        "ast_nodes": ast_nodes,
        "ast_node_count": ast_node_count,
        "checks": checks,
    }


def _dispatch(request: object, protocol_in: Any, protocol_out: Any) -> dict[str, Any]:
    if not isinstance(request, dict):
        raise TypeError("request must be a JSON object")
    operation = request.get("op")
    if operation == "hello":
        return {"kind": "hello", "protocol": 1}
    if operation == "environment_info":
        return {
            "kind": "environment_info",
            "implementation": platform.python_implementation().lower(),
            "python_version": platform.python_version(),
            "platform": platform.platform(),
            "generated_at": datetime.now(timezone.utc).isoformat(),
        }
    if operation == "generator_info":
        return {"kind": "generator_info", "version": _generator_identity()}
    if operation == "minimizer_info":
        return {"kind": "minimizer_info", "version": _minimizer_identity()}
    if operation == "catalog":
        return {"kind": "catalog", "probes": [asdict(spec) for spec in BASELINE_PROBES]}
    if operation == "oracle":
        return _oracle(request)
    if operation == "generate":
        return _generate(request)
    if operation == "minimize":
        return _minimize(request, protocol_in, protocol_out)
    if operation == "shutdown":
        return {"kind": "shutdown"}
    raise ValueError(f"unknown worker operation: {operation!r}")


def main() -> int:
    protocol_in = sys.stdin
    protocol_out = sys.stdout
    while line := protocol_in.readline(_MAX_PROTOCOL_BYTES + 1):
        request: object = None
        try:
            if len(line.encode()) > _MAX_PROTOCOL_BYTES or not line.endswith("\n"):
                raise RuntimeError("worker request exceeds the protocol limit")
            request = json.loads(line)
            captured_stdout = io.StringIO()
            captured_stderr = io.StringIO()
            with contextlib.redirect_stdout(captured_stdout), contextlib.redirect_stderr(
                captured_stderr
            ):
                response = _dispatch(request, protocol_in, protocol_out)
            response["worker_stdout"] = captured_stdout.getvalue()
            response["worker_stderr"] = captured_stderr.getvalue()
            envelope = {"ok": True, "result": response}
        except BaseException as exc:
            envelope = {
                "ok": False,
                "error_type": type(exc).__name__,
                "error_message": str(exc),
            }
        protocol_out.write(json.dumps(envelope, ensure_ascii=False, sort_keys=True) + "\n")
        protocol_out.flush()
        if isinstance(request, dict) and request.get("op") == "shutdown":
            return 0
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
