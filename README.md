# monty-compat

Release-aware Python source transpilation and capability discovery for the
[Monty](https://github.com/pydantic/monty) interpreter.

`monty-compat` learns the exact surface of a released Monty runtime, records
that evidence in a versioned manifest, and lowers unsupported Python into
semantically equivalent constructs only when the rewrite can be proven safe.
When Monty's supported feature set cannot preserve observable Python behavior,
the transpiler raises an explicit error instead of inventing semantics.

[Documentation](https://fswair.github.io/monty-compat/) ·
[Python API](https://fswair.github.io/monty-compat/python-api/) ·
[Release selection](https://fswair.github.io/monty-compat/releases/) ·
[Changelog](https://github.com/fswair/monty-compat/blob/main/CHANGELOG.md)

## Why this exists

Monty intentionally implements a Python subset. File names and Rust symbols can
tell us that `pathlib.Path` exists, but not whether a nested surface such as
`pathlib.Path.is_dir` is available or whether a syntax form parses, type-checks,
and behaves like CPython. `monty-compat` combines three evidence sources:

1. **Static extraction** scans Monty's Rust source for builtins, exceptions,
   modules, module attributes, constructors, and runtime-type attributes.
2. **Behavioral discovery** runs atomic probes on both CPython and the exact
   linked Monty release and classifies the result.
3. **Conservative lowering** rewrites only evidence-backed seams for which the
   required Python semantics can be represented by Monty's supported subset.

Discovery is an offline release-maintenance operation. Default `verified`
transpilation does not download Monty, run probes, start Python workers, or
execute the input. The opt-in `latest` mode performs one bounded, validated
manifest-channel resolution per process.

## Published packages

| Registry | Package | Purpose |
|---|---|---|
| PyPI | `monty-compat` | Python API and native PyO3 transpiler |
| crates.io | `monty-compat` | Rust lowering engine and `monty-lower` CLI |
| crates.io | `monty-compat-extract` | Rust extractor and `monty-extract` CLI |

`monty-compat-python` and `monty-compat-discover` are internal workspace crates
and are not published separately. All Python, Rust, manifests, documentation,
and release automation live in this repository.

## Installation

Python runtime:

```bash
pip install monty-compat
```

Python discovery tooling, including exact Monty, `pysource-codegen`, and
`pysource-minimize`:

```bash
pip install 'monty-compat[discovery]'
```

Rust embedding:

```bash
cargo add monty-compat@0.5.0
cargo add monty-compat-extract@0.5.0
```

The Python wheel uses `abi3` and supports CPython 3.10 and newer.

## Quick start: transpile and run

The public Python hot path is intentionally one function:

```python
from monty_compat import transpiler
from pydantic_monty import Monty

source = """
value = 2
match value:
    case 2:
        result = f"ok:{value}"
    case _:
        result = "other"
result
"""

lowered = transpiler(source)  # release="verified"

with Monty() as pool:
    with pool.checkout() as session:
        assert session.feed_run(lowered) == "ok:2"
```

Pin the exact bundled manifest when reproducibility matters:

```python
lowered = transpiler(source, release="0.0.19")
assert lowered == transpiler(source, release="v0.0.19")
```

`verified` is the default and resolves to the newest manifest compiled into the
installed wheel (`0.0.19` in version 0.5.0). It is deterministic and performs
no network access. `latest` is an explicit freshness mode:

```python
lowered = transpiler(source, release="latest")
```

It downloads the bounded manifest channel from the project documentation,
accepts only a SHA-256-matching manifest that declares compatibility with the
installed lowering engine, and caches the resulting transpiler for the process.
An unavailable, incompatible, or malformed channel fails closed. Exact bundled
versions such as `0.0.19` and `v0.0.19` remain offline.

The function returns ordinary Python source, so `monty-compat` does not wrap,
own, or replace Monty's execution API.

### Failure is explicit

```python
from monty_compat import TranspilationError, transpiler

try:
    transpiler("def values():\n    yield 1\n", release="0.0.19")
except TranspilationError as exc:
    print(exc)
```

`TranspilationError` covers:

- an unbundled release;
- failure to resolve or validate the opt-in `latest` channel;
- invalid Python input;
- manifest validation failure;
- `needs_review` or `not_lowerable` diagnostics for semantics Monty cannot
  currently represent.

The Python binding releases the GIL while Rust parses and lowers the source.
Successful exact-source results are retained in a process-global, release-pinned,
bounded cache. Failures are never cached and cache entries never cross manifests.

## Performance

Release-mode measurements below use a 100 KiB source, Rust 1.95.0, and an Apple
M1 MacBook Air. `p50` and median are the same statistic and are both shown
because release reports expose both labels.

| Operation | Samples | p50 | Median | p99 |
|---|---:|---:|---:|---:|
| Supported source, cache disabled | 200 | 4.224 ms | 4.224 ms | 5.661 ms |
| Supported source, exact cache hit | 20,000 | 0.0317 ms | 0.0317 ms | 0.0424 ms |
| Match-heavy lowering, cache disabled | 200 | 30.957 ms | 30.957 ms | 32.102 ms |
| Match-heavy lowering, exact cache hit | 20,000 | 0.0316 ms | 0.0316 ms | 0.0429 ms |
| Monty 0.0.19 local source extraction | 200 | 113.419 ms | 113.419 ms | 132.644 ms |
| Monty 0.0.19 in-memory ZIP extraction | 200 | 119.257 ms | 119.257 ms | 136.816 ms |

These are local measurements, not latency guarantees. See
[Benchmark methodology](https://github.com/fswair/monty-compat/blob/main/docs/benchmarks.md) for workloads, commands, raw
results, cache-miss interpretation, and reproduction notes.

## Capability graph API

Use `MontyCapabilities` to inspect the exact static surface extracted from
Monty's Rust source:

```python
from monty_compat import MontyCapabilities

caps = MontyCapabilities.from_github()  # latest released source
# caps = MontyCapabilities.from_github(only_released=False)  # main branch
# caps = MontyCapabilities.from_local("/path/to/monty")

assert "pathlib" in caps.modules
assert caps.supports_path("pathlib.Path")
assert caps.supports_path("pathlib.Path.is_dir")

path_methods = caps.get_attributes("pathlib.Path")
print(sorted(path_methods))
print(caps.summary())
```

This graph answers precise questions about known paths; it does not classify a
complete Python program or prove that a snippet can run. Use `transpiler(...)`
for the fail-closed source compatibility decision.

The graph contains:

- `builtin_functions`
- `type_constructors`
- `exception_types`
- `modules`
- `module_attributes`
- `type_attributes`, including paths such as `str.upper`,
  `pathlib.Path.is_dir`, and `re.Pattern.search`

See the [Python API reference](https://github.com/fswair/monty-compat/blob/main/docs/python-api.md) for every public function,
argument, return value, limitation, and error.

## Lowering contract

Every non-supported feature in the bundled manifest has one auditable outcome:

| Availability | Meaning |
|---|---|
| `automatic` | Every represented occurrence has a semantics-preserving rewrite. |
| `contextual` | Rewriting is allowed only when conservative static facts prove its preconditions. |
| `not_lowerable` | Monty's current surface cannot preserve the required behavior. |

Current lowering families include:

- literal, sequence, mapping, OR, guarded, and selected class `match` cases;
- function decorators and complex `for`/`with` targets;
- selected class, descriptor, dataclass, and user-class protocol seams;
- percent formatting, `str.format`, and custom f-string formatting;
- dict union, assert messages, Unicode decimal conversion, and static bytes;
- contextual repairs for selected lazy iterators, class-comprehension scope,
  closure late binding, `async with`, and `asyncio.gather` behavior.

Unsupported semantics remain explicit. The engine does not synthesize exception
inheritance, traceback objects, generator suspension, exception groups, runtime
type mutation, or dispatch precedence that Monty does not implement.

See [Lowering semantics](https://github.com/fswair/monty-compat/blob/main/docs/lowering.md) for before/after examples,
diagnostics, contextual preconditions, and the non-goals that protect semantic
correctness.

## Rust API

The core Rust API accepts a caller-supplied exact manifest:

```rust
use monty_compat::{CacheConfig, CapabilityIndex, Transpiler};

let manifest = std::fs::read_to_string("manifests/monty-v0.0.19.json")?;
let capabilities = CapabilityIndex::from_json(&manifest)?;
let transpiler = Transpiler::with_cache_config(
    capabilities,
    CacheConfig::default(),
);

let output = transpiler.transpile(
    "value = 1\nmatch value:\n    case 1:\n        result = 'one'\nresult\n",
)?;

assert!(output.changed);
assert_eq!(output.target_tag, "v0.0.19");
println!("{}", output.code);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Transpiler` is thread-safe, owns an immutable manifest index, and returns an
`Arc<LoweringOutput>` so exact cache hits reuse the canonical artifact. Use
`CacheConfig::disabled()` for deterministic cache-free measurements.

The extractor is a separate public crate:

```rust
use monty_compat_extract::{extract_release, resolve_release};

let release = resolve_release("0.0.19")?;
let graph = extract_release(&release)?;
assert!(graph.modules.contains("pathlib"));
# Ok::<(), Box<dyn std::error::Error>>(())
```

See the [Rust API and CLI reference](https://github.com/fswair/monty-compat/blob/main/docs/rust-api.md) for public structs,
errors, caching, extraction bounds, and command exit behavior.

## Command-line tools

Lower a file and retain a machine-readable diagnostic report:

```bash
cargo run -p monty-compat -- \
  --manifest manifests/monty-v0.0.19.json \
  --input example.py \
  --output example.lowered.py \
  --report lowering-report.json \
  --deny-needs-review
```

`monty-lower` exits `2` when any seam is `needs_review` or `not_lowerable`
under `--deny-needs-review`, and `1` for loading/parsing failures.

Extract a static graph from a checkout or ZIP archive:

```bash
cargo run -p monty-compat-extract -- \
  --root /path/to/monty \
  --output monty-static-capabilities.json

cargo run -p monty-compat-extract -- \
  --archive monty-v0.0.19.zip \
  --output monty-static-capabilities.json
```

ZIP inputs and downloads are size-bounded, validated, and scanned in memory;
archives are never unpacked onto the filesystem.

## Exact-release discovery

The recommended release pipeline is Rust-orchestrated and links the exact
Monty version being measured:

```bash
cargo run --release --manifest-path crates/monty-discover/Cargo.toml -- \
  --release 0.0.19 \
  --seeds 1000 \
  --python .venv/bin/python \
  --output manifests/monty-v0.0.19.json
```

The pipeline:

1. resolves the requested release and downloads that exact archive;
2. rejects a source/runtime version mismatch;
3. extracts the static graph in Rust;
4. runs baseline semantic probes through killable workers;
5. generates deterministic inert AST corpora with `pysource-codegen`;
6. minimizes failures with `pysource-minimize` while Rust retains the final
   same-fingerprint verdict;
7. atomically writes the versioned manifest.

Generated source is parsed and compiled by CPython, wrapped under `if False`,
and then submitted to Monty. It is never executed directly. Generated failures
are discovery evidence and promotion candidates; they never become “supported”
without a reviewed semantic probe.

See [Discovery and manifest generation](https://github.com/fswair/monty-compat/blob/main/docs/discovery.md) for status classes,
worker boundaries, manifest schema, minimization, and adding a new Monty release.

## Examples

- [Transpile and run with Monty](https://github.com/fswair/monty-compat/blob/main/examples/python/transpile_and_run.py)
- [Handle non-lowerable source](https://github.com/fswair/monty-compat/blob/main/examples/python/error_handling.py)
- [Inspect nested capabilities](https://github.com/fswair/monty-compat/blob/main/examples/python/inspect_capabilities.py)
- [Embed the Rust transpiler](https://github.com/fswair/monty-compat/blob/main/crates/monty-lower/examples/transpile.rs)
- [Inspect a release from Rust](https://github.com/fswair/monty-compat/blob/main/crates/monty-extract/examples/inspect_release.rs)
- [Reproduce benchmarks](https://github.com/fswair/monty-compat/blob/main/docs/benchmarks.md)

## Development and verification

```bash
uv sync --locked --extra dev

cargo +1.95.0 fmt --all -- --check
cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
cargo +1.95.0 test --workspace --locked

cargo +1.95.0 test \
  --manifest-path crates/monty-discover/Cargo.toml \
  --locked

uv run pytest -q
uv run ruff check src tests examples
uv run mypy src

uvx --from zensical==0.0.50 zensical build --clean
```

The differential suite compares original CPython, lowered CPython, and lowered
exact-Monty result/exception envelopes plus stdout and stderr:

```bash
MONTY_COMPAT_CPYTHON=python3.11 \
cargo test --manifest-path crates/monty-discover/Cargo.toml --test differential
```

The workspace forbids Rust `unsafe`. Checked byte ranges, fallible edit planning,
malformed-input tests, exact release fingerprints, and RustSec/cargo-deny checks
are release gates.

## More documentation

- [Documentation site](https://fswair.github.io/monty-compat/)
- [Release selection](https://fswair.github.io/monty-compat/releases/)
- [Python API](https://github.com/fswair/monty-compat/blob/main/docs/python-api.md)
- [Rust API and CLIs](https://github.com/fswair/monty-compat/blob/main/docs/rust-api.md)
- [Lowering semantics](https://github.com/fswair/monty-compat/blob/main/docs/lowering.md)
- [Discovery pipeline](https://github.com/fswair/monty-compat/blob/main/docs/discovery.md)
- [Benchmarks](https://github.com/fswair/monty-compat/blob/main/docs/benchmarks.md)
- [Examples](https://github.com/fswair/monty-compat/blob/main/examples/README.md)
- [Changelog](https://github.com/fswair/monty-compat/blob/main/CHANGELOG.md)

## License

MIT
