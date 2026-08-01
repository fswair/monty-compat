# Rust API and CLI reference

The Rust surface is split into two public crates and two internal workspace
crates.

## Contents

- [`monty-compat`](#monty-compat)
- [Core data types](#core-data-types)
- [Caching](#caching)
- [Diagnostics and errors](#diagnostics-and-errors)
- [`monty-compat-extract`](#monty-compat-extract)
- [Command-line tools](#command-line-tools)
- [Safety properties](#safety-properties)

## Crate map

| Cargo package | Rust crate | Published | Role |
|---|---|---:|---|
| `monty-compat` | `monty_compat` | yes | Manifest-gated lowering and cache |
| `monty-compat-extract` | `monty_compat_extract` | yes | Static source extraction |
| `monty-compat-python` | `_native` | no | Private PyO3 boundary |
| `monty-compat-discover` | `monty_compat_discover` | no | Exact-Monty release probe orchestration |

## `monty-compat`

```toml
[dependencies]
monty-compat = "0.5.0"
```

The core crate does not choose a Monty release. Supply a versioned discovery
manifest and retain the resulting `Transpiler` for repeated calls.

```rust
use monty_compat::{CapabilityIndex, Transpiler};

let index = CapabilityIndex::from_path("manifests/monty-v0.0.19.json")?;
let transpiler = Transpiler::new(index);
let output = transpiler.transpile("value = 1\nvalue\n")?;
assert!(!output.changed);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `CapabilityIndex`

`CapabilityIndex` is the minimal immutable view of a discovery manifest used by
lowering rules.

```rust
let index = CapabilityIndex::from_json(manifest_json)?;
let index = CapabilityIndex::from_path(path)?;

index.target();
index.feature_status("match.literal");
index.feature_statuses();
index.is_parse_unsupported("match.literal");
index.is_not_supported("match.literal");
```

Manifest loading rejects a target tag/runtime-version mismatch. Rules query
stable feature IDs; absence of matching evidence prevents the rule from
running.

### `lower_source`

```rust
use monty_compat::lower_source;

let output = lower_source(source, &index)?;
```

Use this stateless function for a single call. Use `Transpiler` when repeated
exact sources should benefit from caching.

### `Transpiler`

```rust
let transpiler = Transpiler::new(index);
let transpiler = Transpiler::with_cache_config(index, CacheConfig::new(64, 8 << 20));
let transpiler = Transpiler::from_manifest_json(json)?;
let transpiler = Transpiler::from_manifest_path(path)?;

let target = transpiler.target();
let output = transpiler.transpile(source)?;
let stats = transpiler.cache_stats();
transpiler.clear_cache();
```

`Transpiler::transpile` returns `Arc<LoweringOutput>`. A cache hit returns the
same canonical `Arc`. The type is safe to share across threads; parsing and
lowering do not hold the cache mutex.

## Core data types

### `TargetFingerprint`

```rust
pub struct TargetFingerprint {
    pub tag: String,
    pub runtime_version: Option<String>,
}
```

### `LoweringOutput`

```rust
pub struct LoweringOutput {
    pub code: String,
    pub changed: bool,
    pub target_tag: String,
    pub diagnostics: Vec<LoweringDiagnostic>,
}
```

`changed` reports source edits, while diagnostics explain both applied and
deliberately rejected rule decisions.

### `LoweringDiagnostic`

```rust
pub struct LoweringDiagnostic {
    pub rule: &'static str,
    pub disposition: DiagnosticDisposition,
    pub start: usize,
    pub end: usize,
    pub message: String,
}
```

Ranges are UTF-8 byte offsets into the source version inspected by that
lowering pass.

### `DiagnosticDisposition`

| Variant | Serialized value | Meaning |
|---|---|---|
| `Applied` | `applied` | A proven-safe rewrite was emitted. |
| `NeedsReview` | `needs_review` | The feature was found but static preconditions were insufficient. |
| `NotLowerable` | `not_lowerable` | Monty's surface cannot preserve the required semantics. |

The low-level Rust API returns all dispositions. Applications that require the
same fail-closed behavior as Python must reject every disposition other than
`Applied`.

### Lowering coverage

```rust
use monty_compat::{lowering_coverage, LOWERING_COVERAGE};

for feature in lowering_coverage() {
    println!("{}: {:?}", feature.feature, feature.availability);
}
```

`LoweringAvailability` is `Automatic`, `Contextual`, or `NotLowerable`. CI
compares this table with every non-supported feature in the committed manifest.

## Caching

### `CacheConfig`

```rust
let defaults = CacheConfig::default();       // 256 entries, ~32 MiB
let bounded = CacheConfig::new(32, 4 << 20);
let disabled = CacheConfig::disabled();
```

Setting either bound to zero disables caching. `max_bytes` counts retained
source, output buffers, diagnostics, and fixed cache metadata; allocator and
hash-table bookkeeping are implementation-specific.

### `CacheStats`

| Field | Meaning |
|---|---|
| `hits` | Exact-source lookups served from cache. |
| `misses` | Enabled-cache lookups requiring lowering. |
| `insertions` | Successfully retained results. |
| `evictions` | LRU entries removed to satisfy a bound. |
| `skipped` | Successful results too large to retain. |
| `bypasses` | Calls made with caching disabled. |
| `entries` | Current retained entry count. |
| `bytes` | Current retained-size estimate. |

Counters are cumulative. `entries` and `bytes` are the current observational
snapshot.

## Diagnostics and errors

| Error | Cause |
|---|---|
| `ManifestError::Io` | Manifest could not be read. |
| `ManifestError::Json` | Schema/JSON deserialization failed. |
| `ManifestError::VersionMismatch` | Tag and runtime version disagree. |
| `LoweringError::Parse` | Input or generated output is not valid for the pinned parser. |
| `LoweringError::Edit` | Checked source-edit planning failed. |
| `LoweringError::NonConvergent` | Rules did not stabilize within 128 passes. |
| `LoweringError::HelperInjection` | Compatibility helpers could not be inserted safely. |

Errors implement `std::error::Error`; production paths do not require
`unwrap`, `expect`, or panic-based control flow.

## `monty-compat-extract`

```toml
[dependencies]
monty-compat-extract = "0.5.0"
```

### High-level functions

| Function | Input | Result |
|---|---|---|
| `extract_local` | Monty repository root | `CapabilityGraph` |
| `extract_zip` | GitHub-style archive bytes | `CapabilityGraph` |
| `resolve_release` | `latest`, `0.0.19`, or `v0.0.19` | `ReleaseMetadata` |
| `extract_release` | exact `ReleaseMetadata` | downloaded/scanned `CapabilityGraph` |
| `extract_github` | archive/main URL and release policy | `CapabilityGraph` |
| `to_json_pretty` | capability graph | deterministic JSON |

```rust
use monty_compat_extract::{extract_release, resolve_release, to_json_pretty};

let release = resolve_release("0.0.19")?;
let graph = extract_release(&release)?;
let json = to_json_pretty(&graph)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### `ReleaseMetadata`

Carries repository, tag, normalized runtime version, publication metadata,
release page, and exact archive URL. Release tags are normalized and restricted
to URL-safe ASCII before they are interpolated into a URL.

### `CapabilityGraph`

Uses `BTreeSet` and `BTreeMap` fields for deterministic ordering:

- `builtin_functions`
- `type_constructors`
- `exception_types`
- `modules`
- `module_attributes`
- `type_attributes`

### Input bounds

- downloaded archive: 64 MiB compressed maximum;
- ZIP entry count: 100,000 maximum;
- one expanded file: 32 MiB maximum;
- total expanded archive: 256 MiB maximum;
- release metadata response: 1 MiB maximum;
- HTTP timeout: 30 seconds;
- redirects: 5 maximum;
- response headers: 32 KiB maximum.

ZIP paths are validated before scanning. The archive is never unpacked to disk.

## Command-line tools

### `monty-lower`

```text
monty-lower \
  --manifest <manifest.json> \
  [--input <source.py>|-] \
  [--output <lowered.py>|-] \
  [--report <report.json>] \
  [--deny-needs-review]
```

Input and output default to stdin/stdout. Exit codes:

| Code | Meaning |
|---:|---|
| `0` | Lowering completed; no denied diagnostic. |
| `1` | Manifest, input, parse, edit, or I/O error. |
| `2` | `--deny-needs-review` encountered `needs_review` or `not_lowerable`. |

### `monty-extract`

```text
monty-extract (--root <monty-root> | --archive <source.zip>)
              [--output <capabilities.json>]
```

Exactly one input is required. JSON is written to stdout when `--output` is
omitted.

### `monty-discover` (workspace-only)

```bash
cargo run --release --manifest-path crates/monty-discover/Cargo.toml -- \
  --release 0.0.19 --seeds 1000 \
  --python .venv/bin/python \
  --output manifests/monty-v0.0.19.json
```

This binary is intentionally unpublished because it links the exact Monty
release under test and coordinates the Python oracle/generator worker.

## Safety properties

- Workspace lint: `unsafe_code = "forbid"`.
- Source ranges are checked before editing.
- ZIP and HTTP inputs are bounded.
- Cache poisoning recovers without unwinding.
- A new unsupported manifest feature must be classified in the coverage table.
- Differential fixtures compare CPython and exact Monty outcomes.
- Root and exact-Monty lockfiles are audited independently with `cargo-deny`.
