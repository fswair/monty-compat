# Maintainer workflows

Read the relevant section before changing repository behavior.

## File ownership

| Surface | Primary files |
|---|---|
| Python public API | `src/monty_compat/__init__.py`, `_native.pyi` |
| Static Python capability graph | `src/monty_compat/capabilities.py`, `cache.py` |
| Python oracle/generator worker | `_discovery_worker.py`, `generated_discovery.py`, `probes.py` |
| Hand-authored probes | `probe_catalog*.py` |
| Rust lowering | `crates/monty-lower/src/` |
| Lowering coverage | `crates/monty-lower/src/coverage.rs` |
| Rust extraction | `crates/monty-extract/src/` |
| Exact-release orchestration | `crates/monty-discover/` |
| Python native binding | `crates/monty-compat-python/` |
| Release evidence | `manifests/monty-v*.json` |
| User docs/examples | `README.md`, `docs/`, `examples/`, crate `examples/` |

## Add or change a semantic probe

1. Isolate one observable seam in a deterministic module ending in a value.
2. Ensure CPython parses and executes it without external state.
3. Give it a stable category-prefixed ID.
4. Add it to the appropriate `probe_catalog*.py` file.
5. Run catalog validity and CPython execution tests.
6. Probe exact Monty and inspect normalized status/value/exception evidence.
7. Regenerate the manifest only with source/runtime versions equal.

Do not promote generated source directly. Convert a minimized candidate into a
reviewed probe first.

## Add or change lowering

1. Confirm the feature is non-supported in the exact manifest.
2. Decide the semantic guarantee and list every required static precondition.
3. Add facts conservatively; unknown facts must not become false proof.
4. Plan checked byte edits without overlapping ranges.
5. Use deterministic `_monty_compat_...` helper names.
6. Emit `Applied`, `NeedsReview`, or `NotLowerable` for every encountered seam.
7. Add a source/expected golden fixture.
8. Add an exact differential fixture comparing value/exception and I/O.
9. Add/update `LOWERING_COVERAGE`.
10. Run safety tests with malformed, truncated, and Unicode-heavy input.

Never fake inheritance, exceptions, traceback, generator suspension, exception
groups, runtime type mutation, or reflected-dispatch precedence.

## Add a Monty release

1. Pin exact `monty` and `monty-types` versions in the nested discover workspace.
2. Regenerate `crates/monty-discover/Cargo.lock`.
3. Run exact discovery with at least 1,000 generated seeds.
4. Verify release tag, runtime version, linked Rust version, and manifest target.
5. Review baseline changes, minimized failures, and promotion candidates.
6. Expand semantic probes and lowering coverage.
7. Add the new manifest to the PyO3 build registry.
8. Update `manifests/channel.json`, including its SHA-256 and compatible engine
   list; test `verified`, `latest`, bare version, and `v` tag behavior.
9. Run exact Monty smoke and differential suites.
10. Audit root and nested dependency graphs independently.

## Discovery/minimization boundary

Keep Python where CPython itself is the oracle:

- `ast.parse` and `compile`;
- `pysource-codegen`;
- `pysource-minimize` candidate proposals.

Keep Rust responsible for:

- Monty execution and process isolation;
- timeouts/restarts;
- same-fingerprint minimizer verdicts;
- classification/aggregation;
- exact release identity and atomic manifest writes.

Generated modules must be byte/node bounded, compiled raw, wrapped under
`if False`, compiled again, and never executed.

## Benchmarks

Transpilation:

```bash
CARGO_INCREMENTAL=0 cargo +1.95.0 run --release --locked \
  -p monty-compat --example cache_bench
```

Extraction:

```bash
CARGO_INCREMENTAL=0 cargo +1.95.0 run --release --locked \
  -p monty-compat-extract --example extraction_bench -- ROOT ARCHIVE
```

Record hardware, OS, Rust version, target release, sample/warm-up counts,
source/archive size, p50, median, p99, and exact command. p50 equals median.

## Package boundaries

- PyPI wheel: public Python modules, `_native`, type stub, metadata, SBOM.
- PyPI sdist: Python sources and Rust crates required to build the extension.
- `monty-compat` crate: lowering library, CLI, examples, tests; no Monty runtime.
- `monty-compat-extract` crate: scanner/library, CLI, examples.
- `monty-discover`: repository source only, never published as a crate.
- `pysource-*`: discovery extra only, never runtime hot path.

Before commit/publish, ensure `.cargo-key`, `.venv`, `target`, `dist`, caches,
local extraction snapshots, and credentials are ignored.

## Quality gates

```bash
cargo +1.95.0 fmt --all -- --check
cargo +1.95.0 fmt --manifest-path crates/monty-discover/Cargo.toml --all -- --check
cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.95.0 clippy --manifest-path crates/monty-discover/Cargo.toml \
  --all-targets --locked -- -D warnings
cargo +1.95.0 test --workspace --locked
cargo +1.95.0 test --manifest-path crates/monty-discover/Cargo.toml --locked
uv run pytest -q
uv run ruff check src tests examples
uv run mypy src
```

Also run skill validation, package dry-runs, wheel/sdist content inspection,
RustSec/cargo-deny, and exact-Monty wheel smoke before a tag.
