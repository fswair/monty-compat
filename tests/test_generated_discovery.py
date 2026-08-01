"""Tests for inert, generated acceptance discovery."""

from __future__ import annotations

from typing import Any

import pytest

from monty_compat.generated_discovery import (
    GeneratedProbeConfig,
    GeneratedProbeStatus,
    build_generated_report,
    prepare_inert_source,
    run_generated_probes,
)
from monty_compat.probes import run_on_cpython


class _RecordingRunner:
    def __init__(self, error: BaseException | None = None) -> None:
        self.error = error
        self.sources: list[str] = []

    def run(self, source: str) -> Any:
        self.sources.append(source)
        if self.error is not None:
            raise self.error
        return run_on_cpython(source)


def _generator(seed: int, **kwargs: object) -> str:
    assert kwargs == {"node_limit": 20, "depth_limit": 3, "root_node": "Module"}
    return f"value = {seed}\nraise RuntimeError('must stay inert')"


def test_generated_source_is_executed_only_inside_a_dead_branch() -> None:
    config = GeneratedProbeConfig(seed_start=4, seed_count=2, node_limit=20, depth_limit=3)
    runner = _RecordingRunner()

    results, generator_version = run_generated_probes(config, runner, generate_source=_generator)

    assert generator_version is None
    assert [result.seed for result in results] == [4, 5]
    assert all(result.status is GeneratedProbeStatus.COMPLETED for result in results)
    assert all(result.fully_accepted for result in results)
    assert all(source.startswith("if False:") for source in runner.sources)
    assert all(result.source and "must stay inert" in result.source for result in results)


def test_generated_parse_rejection_and_generation_error_are_separate() -> None:
    parser_error_type = type("MontyRuntimeError", (RuntimeError,), {})
    rejected_runner = _RecordingRunner(
        parser_error_type("syntax parser does not yet support generated node")
    )
    config = GeneratedProbeConfig(seed_count=1, node_limit=20, depth_limit=3)

    rejected, _ = run_generated_probes(config, rejected_runner, generate_source=_generator)
    invalid, _ = run_generated_probes(
        config,
        _RecordingRunner(),
        generate_source=lambda seed, **kwargs: "return 1",
    )

    assert rejected[0].status is GeneratedProbeStatus.UNSUPPORTED_PARSE
    assert rejected[0].fully_accepted is False
    assert invalid[0].status is GeneratedProbeStatus.GENERATION_ERROR
    assert invalid[0].fully_accepted is False


def test_generated_report_aggregates_seed_and_ast_evidence() -> None:
    config = GeneratedProbeConfig(seed_count=1, node_limit=20, depth_limit=3)
    results, _ = run_generated_probes(config, _RecordingRunner(), generate_source=_generator)

    report = build_generated_report(config, results, generator_version="0.7.1")

    assert report["generator"]["version"] == "0.7.1"
    assert report["safety"]["raw_generated_code_executed"] is False
    assert report["summary"]["completed"] == 1
    assert report["fully_accepted"] == 1
    assert report["ast_node_outcomes"]["Raise"] == {"completed": 1}


def test_generated_guard_bounds_source_and_ast() -> None:
    with pytest.raises(ValueError, match="max_source_bytes"):
        prepare_inert_source(
            "value = 1",
            GeneratedProbeConfig(max_source_bytes=2),
        )
    with pytest.raises(ValueError, match="max_ast_nodes"):
        prepare_inert_source(
            "value = 1",
            GeneratedProbeConfig(max_ast_nodes=1),
        )
