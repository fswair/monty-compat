---
name: monty-compat
description: Safely transpile, inspect, benchmark, debug, extend, and release Python compatibility for the Monty interpreter using versioned capability manifests, Rust lowering, static extraction, and exact-runtime probes. Use when Codex needs to convert Python for Monty, diagnose unsupported syntax or attributes, query builtins/modules/type methods, use the Python or Rust APIs, add or review a lowering rule, generate a Monty release manifest, work with pysource discovery/minimization, run compatibility benchmarks, or modify the fswair/monty-compat repository.
---

# Monty Compat

Work from evidence tied to an exact Monty release. Preserve Python semantics or
leave the seam unsupported; never invent a convenient approximation.

## Establish context

1. Locate the repository root from the current file or with `git rev-parse`.
2. Read the public API details in `references/api.md` when calling or exposing
   an API.
3. Read `references/maintainer-workflows.md` before changing probes, manifests,
   lowering, extraction, packaging, benchmarks, or release automation.
4. Prefer the repository's `docs/` and executable `examples/` as the canonical
   user-facing explanation.

## Choose the correct surface

| Goal | Surface |
|---|---|
| Produce runnable Monty-compatible source | Python `transpiler(code, release)` or Rust `Transpiler` |
| Execute lowered source | Monty's own `Monty` API |
| Query nested surfaces such as `pathlib.Path.is_dir` | `MontyCapabilities` |
| Extract a static graph | `monty-compat-extract` / `monty-extract` |
| Generate or expand release evidence | workspace-only `monty-discover` |
| Quickly inspect a snippet | `scripts/check_snippet.py` |

Do not run discovery in an end-user request path.

## Transpile a snippet

Use the offline, fail-closed default:

```python
from monty_compat import transpiler

lowered = transpiler(source, release="verified")
```

Or run the helper from the skill directory:

```bash
python scripts/check_snippet.py --release 0.0.19 --file input.py
```

The helper emits JSON and returns `2` for `TranspilationError`. Treat that as a
real compatibility failure. Do not execute the original source as fallback.

After lowering, parse the output and, when Monty is available, run it through
the exact target runtime. For semantic work compare original CPython, lowered
CPython, and lowered Monty outcomes, including exception type/message and
stdout/stderr.

## Inspect a capability

Use canonical paths:

```python
caps.supports_path("pathlib.Path.is_dir")
caps.get_attributes("pathlib.Path")
```

Resolve only evidence present in source extraction. Unknown user-defined
receivers are unknown, not unsupported. Distinguish module attributes from
runtime-type attributes. The graph does not classify complete source; use
`transpiler` for the fail-closed compatibility decision.

## Modify lowering

Follow this order:

1. Identify or add a minimal hand-authored semantic probe.
2. Confirm exact CPython/Monty evidence and stable feature ID.
3. Classify the feature `Automatic`, `Contextual`, or `NotLowerable`.
4. Implement the narrowest conservative facts and rewrite.
5. Emit a diagnostic for every encountered candidate.
6. Add golden and exact-Monty differential fixtures.
7. Update `LOWERING_COVERAGE` for every manifest seam.
8. Run formatting, Clippy, safety tests, differential tests, and Python tests.

Generated `pysource-codegen` findings are promotion candidates, not semantic
proof. Require a reviewed atomic probe before adding supported capability or a
lowering.

## Generate a release manifest

Use the Rust exact-release pipeline. The Monty and Monty-types Cargo versions
must equal the release being probed. A mismatch is a hard error.

```bash
cargo run --release --manifest-path crates/monty-discover/Cargo.toml -- \
  --release 0.0.19 --seeds 1000 \
  --python .venv/bin/python \
  --output manifests/monty-v0.0.19.json
```

Keep `pysource-codegen` and `pysource-minimize` in discovery only. Generated
source must be CPython-parsed/compiled, placed under `if False`, compiled again,
and never executed directly.

## Benchmark

Use release builds, disable incremental compilation, warm the operation, retain
sample counts, and report p50/median/p99. Note that p50 equals median.

```bash
CARGO_INCREMENTAL=0 cargo run --release --locked \
  -p monty-compat --example cache_bench
```

Do not silently replace results in `docs/benchmarks.md`; record environment,
source size, iterations, workload definition, and exact command.

## Guardrails

- Keep end-user Python API simple: `transpiler(code, release="verified")`; do not
  expose a required Python `Transpiler` class.
- Treat `latest` as an explicit network operation. Accept only the bounded,
  hash-checked published channel when it declares the current engine compatible;
  never fall back silently to `verified`.
- Never wrap or take ownership of Monty's execution API.
- Never label evidence from one Monty version as another.
- Never synthesize inheritance, traceback, generator suspension, exception
  groups, runtime type mutation, or dispatch behavior absent from Monty.
- Keep production Rust free of `unsafe`, panic-based control flow, and unchecked
  source ranges.
- Keep network/probe/generator work outside the default `verified`
  transpilation hot path. The explicit `latest` mode may resolve its bounded,
  validated manifest channel once before lowering.
- Preserve unrelated work in a dirty tree and never commit local keys, build
  output, caches, or inspection snapshots.
- Keep `CRATES_IO_TOKEN` and all publishing credentials out of source and docs.

## Validate before handoff

Run the proportional subset, and run the full matrix before a release:

```bash
cargo +1.95.0 fmt --all -- --check
cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.95.0 test --workspace --locked
cargo +1.95.0 test --manifest-path crates/monty-discover/Cargo.toml --locked
uv run pytest -q
uv run ruff check src tests examples
uv run mypy src
```

Also validate both Cargo lockfiles with their `cargo-deny` configurations before
publishing.

## Resource routing

- Read `references/api.md` for public Python/Rust signatures, errors, cache
  semantics, and CLI exit codes.
- Read `references/maintainer-workflows.md` for file ownership, probe/lowering
  changes, exact-release generation, packaging, benchmarks, and test gates.
- Execute `scripts/check_snippet.py` for deterministic JSON transpilation output.
