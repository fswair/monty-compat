"""Behavioral feature probes for Monty's Python language surface."""

from __future__ import annotations

import ast
import sys
from collections import Counter
from collections.abc import Iterator, Mapping, Sequence
from dataclasses import asdict, dataclass
from enum import Enum
from typing import Any, Protocol


class ProbeStatus(str, Enum):
    """Stable classifications emitted by the behavioral probe runner."""

    SUPPORTED = "supported"
    UNSUPPORTED_PARSE = "unsupported_parse"
    UNSUPPORTED_TYPE_CHECK = "unsupported_type_check"
    UNSUPPORTED_RUNTIME = "unsupported_runtime"
    SEMANTIC_MISMATCH = "semantic_mismatch"
    CRASH = "crash"
    TIMEOUT = "timeout"
    INVALID_PROBE = "invalid_probe"
    UNKNOWN_ERROR = "unknown_error"


@dataclass(frozen=True)
class ProbeSpec:
    """One atomic Python feature probe whose final expression is its result."""

    id: str
    category: str
    source: str
    description: str
    minimum_python: tuple[int, int] = (3, 10)


def is_probe_supported_by_host(spec: ProbeSpec) -> bool:
    """Return whether the running CPython can parse and execute *spec*."""
    return sys.version_info[:2] >= spec.minimum_python


@dataclass(frozen=True)
class ProbeResult:
    """Monty's outcome compared with the same probe on CPython."""

    id: str
    category: str
    description: str
    status: ProbeStatus
    ast_nodes: tuple[str, ...] = ()
    expected: Any = None
    actual: Any = None
    error_type: str | None = None
    error_message: str | None = None

    def to_dict(self) -> dict[str, Any]:
        """Return a JSON-safe representation."""
        data = asdict(self)
        data["status"] = self.status.value
        return data


class ProbeRunner(Protocol):
    """Runtime adapter used by the discovery engine."""

    def run(self, source: str) -> Any:
        """Execute *source* and return its final expression."""


class PydanticMontyRunner:
    """Run probes against the installed ``pydantic-monty`` worker pool."""

    def __init__(self, *, request_timeout: float = 2.0) -> None:
        self.request_timeout = request_timeout
        self._runtime: Any = None

    def __enter__(self) -> PydanticMontyRunner:
        try:
            from pydantic_monty import Monty
        except ImportError as exc:  # pragma: no cover - environment dependent
            raise RuntimeError(
                "behavioral discovery requires the 'pydantic-monty' package"
            ) from exc
        self._runtime = Monty(
            min_processes=1,
            max_processes=1,
            request_timeout=self.request_timeout,
        )
        self._runtime.__enter__()
        return self

    def __exit__(self, exc_type: object, exc: object, traceback: object) -> None:
        if self._runtime is not None:
            self._runtime.__exit__(exc_type, exc, traceback)
            self._runtime = None

    def run(self, source: str) -> Any:
        if self._runtime is None:
            raise RuntimeError("PydanticMontyRunner must be used as a context manager")
        with self._runtime.checkout() as session:
            return session.feed_run(source, inputs={})


def run_on_cpython(source: str) -> Any:
    """Execute trusted package-owned probe source and return its final expression."""
    tree = ast.parse(source, filename="<monty-capability-probe>", mode="exec")
    # Dataclass internals consult ``sys.modules[cls.__module__]``. Reusing the
    # real main-module identity keeps the oracle faithful without registering a
    # synthetic module globally.
    namespace: dict[str, Any] = {"__name__": "__main__"}
    if tree.body and isinstance(tree.body[-1], ast.Expr):
        prefix = ast.Module(body=tree.body[:-1], type_ignores=tree.type_ignores)
        expression = ast.Expression(tree.body[-1].value)
        exec(compile(prefix, "<monty-capability-probe>", "exec"), namespace)
        return eval(compile(expression, "<monty-capability-probe>", "eval"), namespace)
    exec(compile(tree, "<monty-capability-probe>", "exec"), namespace)
    return None


def _strict_equal(left: Any, right: Any) -> bool:
    """Compare probe values without treating ``True`` and ``1`` as identical."""
    if type(left) is not type(right):
        return False
    if isinstance(left, Mapping):
        return left.keys() == right.keys() and all(
            _strict_equal(left[key], right[key]) for key in left
        )
    if isinstance(left, (list, tuple)):
        return len(left) == len(right) and all(
            _strict_equal(left_item, right_item)
            for left_item, right_item in zip(left, right, strict=True)
        )
    return bool(left == right)


def _json_safe(value: Any) -> Any:
    """Normalize boundary values while preserving useful type information."""
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, bytes):
        return {"type": "bytes", "hex": value.hex()}
    if isinstance(value, tuple):
        return {"type": "tuple", "items": [_json_safe(item) for item in value]}
    if isinstance(value, list):
        return [_json_safe(item) for item in value]
    if isinstance(value, Mapping):
        return {str(key): _json_safe(item) for key, item in value.items()}
    return {"type": type(value).__name__, "repr": repr(value)}


def classify_monty_error(error: BaseException) -> ProbeStatus:
    """Map a Monty adapter exception to a stable capability status."""
    name = type(error).__name__
    message = str(error).lower()
    if name == "MontySyntaxError":
        return ProbeStatus.UNSUPPORTED_PARSE
    if "syntax parser does not yet support" in message:
        return ProbeStatus.UNSUPPORTED_PARSE
    if name == "MontyTypingError":
        return ProbeStatus.UNSUPPORTED_TYPE_CHECK
    if name in {"MontyRuntimeError", "ExternalException"}:
        return ProbeStatus.UNSUPPORTED_RUNTIME
    if name == "MontyCrashedError":
        return ProbeStatus.CRASH
    if isinstance(error, TimeoutError) or "Timeout" in name:
        return ProbeStatus.TIMEOUT
    return ProbeStatus.UNKNOWN_ERROR


def ast_nodes_in(source: str) -> tuple[str, ...]:
    """Return statement/expression AST node names exercised by probe source."""
    tree = ast.parse(source)
    return tuple(
        sorted(
            {
                type(node).__name__
                for node in ast.walk(tree)
                if isinstance(node, (ast.stmt, ast.expr))
            }
        )
    )


def run_probe(spec: ProbeSpec, runner: ProbeRunner) -> ProbeResult:
    """Run one probe on CPython and Monty, then classify the outcome."""
    if not is_probe_supported_by_host(spec):
        required = ".".join(str(part) for part in spec.minimum_python)
        current = ".".join(str(part) for part in sys.version_info[:2])
        return ProbeResult(
            id=spec.id,
            category=spec.category,
            description=spec.description,
            status=ProbeStatus.INVALID_PROBE,
            error_type="UnsupportedPythonVersion",
            error_message=f"probe requires CPython {required}+; running {current}",
        )
    try:
        ast_nodes = ast_nodes_in(spec.source)
    except SyntaxError:
        ast_nodes = ()
    try:
        expected = run_on_cpython(spec.source)
    except BaseException as exc:
        return ProbeResult(
            id=spec.id,
            category=spec.category,
            description=spec.description,
            status=ProbeStatus.INVALID_PROBE,
            ast_nodes=ast_nodes,
            error_type=type(exc).__name__,
            error_message=str(exc),
        )

    try:
        actual = runner.run(spec.source)
    except BaseException as exc:
        return ProbeResult(
            id=spec.id,
            category=spec.category,
            description=spec.description,
            status=classify_monty_error(exc),
            ast_nodes=ast_nodes,
            expected=_json_safe(expected),
            error_type=type(exc).__name__,
            error_message=str(exc),
        )

    status = (
        ProbeStatus.SUPPORTED if _strict_equal(expected, actual) else ProbeStatus.SEMANTIC_MISMATCH
    )
    return ProbeResult(
        id=spec.id,
        category=spec.category,
        description=spec.description,
        status=status,
        ast_nodes=ast_nodes,
        expected=_json_safe(expected),
        actual=_json_safe(actual),
    )


def run_probes(specs: Sequence[ProbeSpec], runner: ProbeRunner) -> list[ProbeResult]:
    """Run an ordered feature catalog against one runtime adapter."""
    return [run_probe(spec, runner) for spec in specs]


def summarize_results(results: Sequence[ProbeResult]) -> dict[str, int]:
    """Count probe classifications for compact manifest reporting."""
    counts = Counter(result.status.value for result in results)
    return {status.value: counts.get(status.value, 0) for status in ProbeStatus}


def summarize_ast_coverage(results: Sequence[ProbeResult]) -> dict[str, int]:
    """Count probes exercising each Python statement/expression AST node."""
    counts = Counter(node for result in results for node in result.ast_nodes)
    return dict(sorted(counts.items()))


def cpython_fingerprint() -> dict[str, Any]:
    """Describe the oracle used to determine expected probe behavior."""
    return {
        "implementation": sys.implementation.name,
        "version": ".".join(str(part) for part in sys.version_info[:3]),
    }


def iter_invalid_catalog_entries(specs: Sequence[ProbeSpec]) -> Iterator[str]:
    """Yield structural catalog problems without executing Monty."""
    seen: set[str] = set()
    for spec in specs:
        if spec.id in seen:
            yield f"duplicate probe id: {spec.id}"
        seen.add(spec.id)
        if not spec.category:
            yield f"{spec.id}: empty category"
        if not is_probe_supported_by_host(spec):
            continue
        try:
            ast.parse(spec.source)
        except SyntaxError as exc:
            yield f"{spec.id}: invalid CPython probe syntax: {exc}"
