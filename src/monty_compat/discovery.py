"""Build a versioned static and behavioral capability manifest for Monty."""

from __future__ import annotations

import argparse
import json
import platform
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from typing import Any
from urllib.request import Request, urlopen

from .capabilities import MontyCapabilities
from .generated_discovery import (
    GeneratedProbeConfig,
    build_generated_report,
    run_generated_probes,
)
from .probe_catalog import BASELINE_PROBES
from .probes import (
    ProbeResult,
    ProbeSpec,
    PydanticMontyRunner,
    cpython_fingerprint,
    iter_invalid_catalog_entries,
    run_probes,
    summarize_ast_coverage,
    summarize_results,
)

_LATEST_RELEASE_API = "https://api.github.com/repos/pydantic/monty/releases/latest"
_MANIFEST_SCHEMA_VERSION = 1
_PROBE_SCHEMA_VERSION = 3


@dataclass(frozen=True)
class ReleaseFingerprint:
    """Immutable identity fields for the Monty source being discovered."""

    repository: str
    tag: str
    published_at: str
    release_url: str

    @property
    def normalized_version(self) -> str:
        """Return a Python-distribution-like version from a release tag."""
        return self.tag.removeprefix("v")


def fetch_latest_release() -> ReleaseFingerprint:
    """Fetch the latest non-prerelease Monty release identity from GitHub."""
    request = Request(
        _LATEST_RELEASE_API,
        headers={"Accept": "application/vnd.github+json", "User-Agent": "monty-compat"},
    )
    with urlopen(request) as response:  # noqa: S310
        payload: object = json.loads(response.read())
    if not isinstance(payload, dict):
        raise RuntimeError("Monty's release API returned a non-object response")

    required = ("tag_name", "published_at", "html_url")
    if not all(isinstance(payload.get(key), str) for key in required):
        raise RuntimeError("Monty's release API response is missing identity fields")
    if payload.get("draft") or payload.get("prerelease"):
        raise RuntimeError("Monty's latest release endpoint returned a draft or prerelease")
    return ReleaseFingerprint(
        repository="pydantic/monty",
        tag=payload["tag_name"],
        published_at=payload["published_at"],
        release_url=payload["html_url"],
    )


def installed_monty_version() -> str | None:
    """Return the installed behavioral runtime version, if available."""
    try:
        return version("pydantic-monty")
    except PackageNotFoundError:
        return None


def build_manifest(
    release: ReleaseFingerprint,
    capabilities: MontyCapabilities,
    results: list[ProbeResult],
    *,
    runtime_version: str | None,
    generated_report: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Combine static extraction and behavioral evidence into one artifact."""
    behavioral: dict[str, Any] = {
        "probe_schema_version": _PROBE_SCHEMA_VERSION,
        "summary": summarize_results(results),
        "ast_node_coverage": summarize_ast_coverage(results),
        "features": {result.id: result.to_dict() for result in results},
    }
    if generated_report is not None:
        behavioral["generated_corpus"] = generated_report

    return {
        "schema_version": _MANIFEST_SCHEMA_VERSION,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "target": {
            **asdict(release),
            "runtime_distribution": "pydantic-monty",
            "runtime_version": runtime_version,
            "build_features": [],
            "platform": platform.platform(),
        },
        "oracle": cpython_fingerprint(),
        "static_capabilities": capabilities.to_dict(),
        "behavioral_capabilities": behavioral,
    }


def discover_latest_release(
    *,
    specs: tuple[ProbeSpec, ...] = BASELINE_PROBES,
    allow_version_mismatch: bool = False,
    generated_config: GeneratedProbeConfig | None = None,
) -> dict[str, Any]:
    """Discover the latest released Monty source and installed runtime behavior."""
    catalog_errors = list(iter_invalid_catalog_entries(specs))
    if catalog_errors:
        raise RuntimeError("invalid probe catalog:\n" + "\n".join(catalog_errors))

    release = fetch_latest_release()
    runtime_version = installed_monty_version()
    if runtime_version is None:
        raise RuntimeError("install the 'pydantic-monty' package to run behavioral discovery")
    if runtime_version != release.normalized_version and not allow_version_mismatch:
        raise RuntimeError(
            f"latest release is {release.normalized_version}, but installed pydantic-monty "
            f"is {runtime_version}; install the matching runtime or pass "
            "allow_version_mismatch=True"
        )

    capabilities = MontyCapabilities.from_github(only_released=True)
    with PydanticMontyRunner() as runner:
        results = run_probes(specs, runner)
        if generated_config is None:
            generated_report = None
        else:
            generated_results, generator_version = run_generated_probes(generated_config, runner)
            generated_report = build_generated_report(
                generated_config,
                generated_results,
                generator_version=generator_version,
            )
    return build_manifest(
        release,
        capabilities,
        results,
        runtime_version=runtime_version,
        generated_report=generated_report,
    )


def write_manifest(manifest: dict[str, Any], output: str | Path) -> Path:
    """Write a deterministic, readable capability manifest."""
    path = Path(output)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def main(argv: list[str] | None = None) -> int:
    """CLI entry point for release discovery."""
    parser = argparse.ArgumentParser(
        prog="monty-compat-discover",
        description="Extract and behaviorally probe the latest released Monty runtime.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("monty-capabilities.json"),
        help="manifest path (default: monty-capabilities.json)",
    )
    parser.add_argument(
        "--allow-version-mismatch",
        action="store_true",
        help="probe an installed runtime that does not match the latest release tag",
    )
    parser.add_argument(
        "--generated-seeds",
        type=int,
        default=0,
        help="also run this many inert pysource-codegen seeds (default: disabled)",
    )
    parser.add_argument(
        "--generated-seed-start",
        type=int,
        default=0,
        help="first deterministic generated seed (default: 0)",
    )
    parser.add_argument(
        "--generated-node-limit",
        type=int,
        default=100,
        help="pysource-codegen node budget per seed (default: 100)",
    )
    parser.add_argument(
        "--generated-depth-limit",
        type=int,
        default=5,
        help="pysource-codegen AST depth budget (default: 5)",
    )
    args = parser.parse_args(argv)

    generated_config = (
        GeneratedProbeConfig(
            seed_start=args.generated_seed_start,
            seed_count=args.generated_seeds,
            node_limit=args.generated_node_limit,
            depth_limit=args.generated_depth_limit,
        )
        if args.generated_seeds
        else None
    )
    manifest = discover_latest_release(
        allow_version_mismatch=args.allow_version_mismatch,
        generated_config=generated_config,
    )
    path = write_manifest(manifest, args.output)
    summary = manifest["behavioral_capabilities"]["summary"]
    print(f"wrote {path} ({sum(summary.values())} probes)")
    print(json.dumps(summary, sort_keys=True))
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
