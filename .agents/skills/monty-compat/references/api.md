# API reference

Use this compact reference when invoking monty-compat. For prose and examples,
read the repository's `docs/python-api.md` and `docs/rust-api.md`.

## Python

### Runtime transpilation

```python
from monty_compat import TranspilationError, transpiler

lowered: str = transpiler(
    code: str,
    release: str | Literal["verified", "latest"] = "verified",
)
```

- `verified`: newest wheel-embedded manifest; offline and reproducible;
- `latest`: bounded published channel plus SHA-256, target, engine, and feature
  compatibility validation; successful resolution is process-cached;
- exact bare versions and `v`-prefixed bundled tags remain offline;
- no probes, extraction, or source execution in any transpilation mode;
- supported source may be returned unchanged;
- unknown releases and any non-applied diagnostic raise
  `TranspilationError`;
- successful exact-source results use a release-scoped bounded native cache;
- GIL is released during Rust parsing/lowering.

### Capability graph

```python
caps = MontyCapabilities.from_local(root)
caps = MontyCapabilities.from_github(only_released=True)
caps = MontyCapabilities.from_dict(data)

caps.get_attributes("pathlib.Path")
caps.supports_path("pathlib.Path.is_dir")
caps.to_dict()
caps.summary()
caps.to_prompt_context()
```

Fields: `builtin_functions`, `type_constructors`, `exception_types`, `modules`,
`module_attributes`, and `type_attributes`.

This is a path/query API, not a complete-source compatibility classifier.

### Discovery exports

```python
GeneratedProbeConfig(
    seed_start=0,
    seed_count=100,
    node_limit=100,
    depth_limit=5,
    max_source_bytes=100_000,
    max_ast_nodes=2_000,
)

manifest = discover_latest_release(
    allow_version_mismatch=False,
    generated_config=None,
)
path = write_manifest(manifest, output)
```

Prefer Rust exact-release discovery for committed manifests.

## Rust: `monty_compat`

```rust
let index = CapabilityIndex::from_json(json)?;
let index = CapabilityIndex::from_path(path)?;

let transpiler = Transpiler::new(index);
let transpiler = Transpiler::with_cache_config(index, CacheConfig::new(entries, bytes));
let output: Arc<LoweringOutput> = transpiler.transpile(source)?;
let stats: CacheStats = transpiler.cache_stats();
transpiler.clear_cache();
```

`LoweringOutput`: `code`, `changed`, `target_tag`, `diagnostics`.

`LoweringDiagnostic`: `rule`, `disposition`, byte `start`/`end`, `message`.

Dispositions: `Applied`, `NeedsReview`, `NotLowerable`. Reject the latter two
for fail-closed execution.

Errors:

- `ManifestError::{Io, Json, VersionMismatch}`
- `LoweringError::{Parse, Edit, NonConvergent, HelperInjection}`

Use `lower_source(source, &index)` for stateless calls. Use
`lowering_coverage()` to inspect all non-supported feature classifications.

## Rust: `monty_compat_extract`

```rust
extract_local(root) -> Result<CapabilityGraph, ExtractError>
extract_zip(bytes) -> Result<CapabilityGraph, ExtractError>
resolve_release(release) -> Result<ReleaseMetadata, ExtractError>
extract_release(&metadata) -> Result<CapabilityGraph, ExtractError>
extract_github(url, only_released) -> Result<CapabilityGraph, ExtractError>
to_json_pretty(&graph) -> Result<String, ExtractError>
```

Archives/downloads are bounded and validated. Extraction does not execute Monty.

## CLIs

```text
monty-lower --manifest M [--input I] [--output O]
            [--report R] [--deny-needs-review]
```

Exit `0` success, `1` error, `2` denied diagnostic.

```text
monty-extract (--root ROOT | --archive ZIP) [--output JSON]
```

Workspace exact-release command:

```text
cargo run --release --manifest-path crates/monty-discover/Cargo.toml --
  --release VERSION [--seeds N] --python PYTHON --output MANIFEST
```
