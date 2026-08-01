"""Deterministic, inert acceptance fuzzing with ``pysource-codegen``."""

from __future__ import annotations

import ast
import hashlib
from collections import Counter
from collections.abc import Callable, Sequence
from dataclasses import asdict, dataclass
from enum import Enum
from importlib.metadata import PackageNotFoundError, version
from typing import Any

from .probes import ProbeRunner, ProbeStatus, classify_monty_error

_GENERATED_SCHEMA_VERSION = 1


class GeneratedProbeStatus(str, Enum):
    """Stable outcomes for generated, non-executing acceptance probes."""

    COMPLETED = "completed"
    UNSUPPORTED_PARSE = ProbeStatus.UNSUPPORTED_PARSE.value
    UNSUPPORTED_TYPE_CHECK = ProbeStatus.UNSUPPORTED_TYPE_CHECK.value
    UNSUPPORTED_RUNTIME = ProbeStatus.UNSUPPORTED_RUNTIME.value
    SEMANTIC_MISMATCH = ProbeStatus.SEMANTIC_MISMATCH.value
    CRASH = ProbeStatus.CRASH.value
    TIMEOUT = ProbeStatus.TIMEOUT.value
    GENERATION_ERROR = "generation_error"
    GUARD_REJECTED = "guard_rejected"
    UNKNOWN_ERROR = ProbeStatus.UNKNOWN_ERROR.value


@dataclass(frozen=True)
class GeneratedProbeConfig:
    """Bounds and deterministic seed range for one generated corpus."""

    seed_start: int = 0
    seed_count: int = 100
    node_limit: int = 100
    depth_limit: int = 5
    max_source_bytes: int = 100_000
    max_ast_nodes: int = 2_000

    def __post_init__(self) -> None:
        for field in ("seed_start", "seed_count", "node_limit", "depth_limit"):
            if getattr(self, field) < 0:
                raise ValueError(f"{field} must not be negative")
        if self.node_limit == 0:
            raise ValueError("node_limit must be positive")
        if self.depth_limit == 0:
            raise ValueError("depth_limit must be positive")
        if self.max_source_bytes <= 0:
            raise ValueError("max_source_bytes must be positive")
        if self.max_ast_nodes <= 0:
            raise ValueError("max_ast_nodes must be positive")


@dataclass(frozen=True)
class GeneratedProbeResult:
    """Outcome for one reproducible generator seed."""

    seed: int
    status: GeneratedProbeStatus
    fully_accepted: bool
    ast_nodes: tuple[str, ...] = ()
    ast_node_count: int = 0
    source_sha256: str | None = None
    source: str | None = None
    error_type: str | None = None
    error_message: str | None = None

    def to_dict(self) -> dict[str, Any]:
        """Return a JSON-safe representation."""
        data = asdict(self)
        data["status"] = self.status.value
        return data


SourceGenerator = Callable[..., str]


class _GeneratedSourceError(ValueError):
    """The generator emitted source that CPython cannot compile."""


class _GeneratedGuardError(ValueError):
    """The generated sample could not be made safely inert."""


def _load_generator() -> tuple[SourceGenerator, str | None]:
    try:
        import pysource_codegen
    except ImportError as exc:  # pragma: no cover - environment dependent
        raise RuntimeError(
            "generated discovery requires 'pysource-codegen'; install 'monty-compat[discovery]'"
        ) from exc

    generator_version = getattr(pysource_codegen, "__version__", None)
    if not isinstance(generator_version, str):
        try:
            generator_version = version("pysource-codegen")
        except PackageNotFoundError:
            generator_version = None
    return pysource_codegen.generate, generator_version


def _ast_nodes(tree: ast.AST) -> tuple[str, ...]:
    return tuple(
        sorted(
            {
                type(node).__name__
                for node in ast.walk(tree)
                if isinstance(node, (ast.stmt, ast.expr))
            }
        )
    )


def prepare_inert_source(
    source: str, config: GeneratedProbeConfig
) -> tuple[str, tuple[str, ...], int]:
    """Validate generated source and put its module body under ``if False``.

    ``pysource-codegen`` explicitly does not promise that its output is safe to
    execute. The wrapper still makes Monty parse and type-check every generated
    node, but no generated statement is evaluated at runtime.
    """
    if len(source.encode("utf-8")) > config.max_source_bytes:
        raise _GeneratedGuardError("generated source exceeds max_source_bytes")

    try:
        tree = ast.parse(source, filename="<pysource-codegen-raw>", mode="exec")
    except (SyntaxError, ValueError, TypeError) as exc:
        raise _GeneratedSourceError(f"ast.parse failed: {exc}") from exc
    try:
        compile(source, "<pysource-codegen-raw>", "exec")
    except (SyntaxError, ValueError, TypeError) as exc:
        raise _GeneratedSourceError(f"compile failed: {exc}") from exc

    node_count = sum(1 for _ in ast.walk(tree))
    if node_count > config.max_ast_nodes:
        raise _GeneratedGuardError(
            f"generated AST has {node_count} nodes; max_ast_nodes is {config.max_ast_nodes}"
        )

    inert_tree = ast.Module(
        body=[
            ast.If(
                test=ast.Constant(value=False),
                body=tree.body or [ast.Pass()],
                orelse=[],
            ),
            ast.Expr(value=ast.Constant(value=None)),
        ],
        type_ignores=[],
    )
    ast.fix_missing_locations(inert_tree)
    try:
        compile(inert_tree, "<pysource-codegen-inert>", "exec")
        inert_source = ast.unparse(inert_tree)
    except (SyntaxError, ValueError, TypeError) as exc:
        raise _GeneratedGuardError(f"cannot safely wrap generated source: {exc}") from exc
    return inert_source, _ast_nodes(tree), node_count


def _generated_status(error: BaseException) -> GeneratedProbeStatus:
    status = classify_monty_error(error)
    try:
        return GeneratedProbeStatus(status.value)
    except ValueError:
        return GeneratedProbeStatus.UNKNOWN_ERROR


def _failed_result(
    seed: int,
    status: GeneratedProbeStatus,
    error: BaseException,
    *,
    source: str | None = None,
    ast_nodes: tuple[str, ...] = (),
    ast_node_count: int = 0,
) -> GeneratedProbeResult:
    source_hash = hashlib.sha256(source.encode()).hexdigest() if source is not None else None
    return GeneratedProbeResult(
        seed=seed,
        status=status,
        fully_accepted=False,
        ast_nodes=ast_nodes,
        ast_node_count=ast_node_count,
        source_sha256=source_hash,
        source=source,
        error_type=type(error).__name__,
        error_message=str(error),
    )


def run_generated_probes(
    config: GeneratedProbeConfig,
    runner: ProbeRunner,
    *,
    generate_source: SourceGenerator | None = None,
) -> tuple[list[GeneratedProbeResult], str | None]:
    """Generate deterministic sources and test their inert forms on Monty."""
    if generate_source is None:
        generate_source, generator_version = _load_generator()
    else:
        generator_version = None

    results: list[GeneratedProbeResult] = []
    for seed in range(config.seed_start, config.seed_start + config.seed_count):
        try:
            source = generate_source(
                seed,
                node_limit=config.node_limit,
                depth_limit=config.depth_limit,
                root_node="Module",
            )
            if not isinstance(source, str):
                raise TypeError("generator returned a non-string source")
        except BaseException as exc:
            results.append(_failed_result(seed, GeneratedProbeStatus.GENERATION_ERROR, exc))
            continue

        try:
            inert_source, ast_nodes, node_count = prepare_inert_source(source, config)
        except _GeneratedSourceError as exc:
            results.append(
                _failed_result(
                    seed,
                    GeneratedProbeStatus.GENERATION_ERROR,
                    exc,
                    source=source,
                )
            )
            continue
        except _GeneratedGuardError as exc:
            results.append(
                _failed_result(
                    seed,
                    GeneratedProbeStatus.GUARD_REJECTED,
                    exc,
                    source=source,
                )
            )
            continue

        source_hash = hashlib.sha256(source.encode()).hexdigest()
        try:
            actual = runner.run(inert_source)
        except BaseException as exc:
            status = _generated_status(exc)
            results.append(
                _failed_result(
                    seed,
                    status,
                    exc,
                    source=source,
                    ast_nodes=ast_nodes,
                    ast_node_count=node_count,
                )
            )
            continue

        status = (
            GeneratedProbeStatus.COMPLETED
            if actual is None
            else GeneratedProbeStatus.SEMANTIC_MISMATCH
        )
        results.append(
            GeneratedProbeResult(
                seed=seed,
                status=status,
                fully_accepted=True,
                ast_nodes=ast_nodes,
                ast_node_count=node_count,
                source_sha256=source_hash,
                source=source,
                error_message=None if actual is None else f"inert source returned {actual!r}",
            )
        )
    return results, generator_version


def summarize_generated_results(
    results: Sequence[GeneratedProbeResult],
) -> dict[str, int]:
    """Count all generated-corpus outcomes in stable enum order."""
    counts = Counter(result.status.value for result in results)
    return {status.value: counts.get(status.value, 0) for status in GeneratedProbeStatus}


def summarize_generated_ast_outcomes(
    results: Sequence[GeneratedProbeResult],
) -> dict[str, dict[str, int]]:
    """Aggregate outcome evidence for every generated statement/expression node."""
    outcomes: dict[str, Counter[str]] = {}
    for result in results:
        for node in result.ast_nodes:
            outcomes.setdefault(node, Counter())[result.status.value] += 1
    return {node: dict(sorted(counts.items())) for node, counts in sorted(outcomes.items())}


def build_generated_report(
    config: GeneratedProbeConfig,
    results: Sequence[GeneratedProbeResult],
    *,
    generator_version: str | None,
) -> dict[str, Any]:
    """Build the standalone generated-corpus section of a capability manifest."""
    return {
        "schema_version": _GENERATED_SCHEMA_VERSION,
        "generator": {
            "distribution": "pysource-codegen",
            "version": generator_version,
        },
        "safety": {
            "mode": "dead_branch",
            "raw_generated_code_executed": False,
            "description": "generated module body is parsed under `if False`",
        },
        "config": asdict(config),
        "summary": summarize_generated_results(results),
        "fully_accepted": sum(result.fully_accepted for result in results),
        "ast_node_outcomes": summarize_generated_ast_outcomes(results),
        "results": [result.to_dict() for result in results],
    }
