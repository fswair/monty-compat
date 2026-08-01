# monty-compat

Fast, release-aware Python source lowering for the
[Monty](https://github.com/pydantic/monty) interpreter.

The crate consumes an exact behavioral capability manifest and rewrites only
unsupported seams whose observable Python semantics can be preserved. Unsafe
or unrepresentable cases remain explicit diagnostics.

## Usage

```toml
[dependencies]
monty-compat = "0.5.0"
```

```rust
use monty_compat::{CapabilityIndex, DiagnosticDisposition, Transpiler};

let capabilities = CapabilityIndex::from_path("monty-v0.0.19.json")?;
let transpiler = Transpiler::new(capabilities);
let output = transpiler.transpile(
    "value = 1\nmatch value:\n    case 1:\n        result = 'one'\nresult\n",
)?;

if output
    .diagnostics
    .iter()
    .any(|diagnostic| diagnostic.disposition != DiagnosticDisposition::Applied)
{
    return Err("source cannot be lowered safely".into());
}

println!("{}", output.code);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Public surface

- `CapabilityIndex`: validated manifest feature/status index.
- `lower_source`: one-shot stateless lowering.
- `Transpiler`: long-lived, release-pinned lowering with bounded exact-source
  caching.
- `LoweringOutput`: code, changed flag, target tag, and diagnostics.
- `DiagnosticDisposition`: `Applied`, `NeedsReview`, or `NotLowerable`.
- `CacheConfig` / `CacheStats`: explicit cache bounds and telemetry.
- `lowering_coverage`: auditable automatic/contextual/not-lowerable matrix.

The low-level API returns diagnostics rather than hiding them. Reject every
non-`Applied` disposition when execution must fail closed.

## Cache

```rust
use monty_compat::{CacheConfig, Transpiler};

let cached = Transpiler::with_cache_config(index, CacheConfig::new(64, 8 << 20));
let uncached = Transpiler::with_cache_config(other_index, CacheConfig::disabled());
```

Default bounds are 256 entries and approximately 32 MiB. Successful exact
source hits reuse one `Arc<LoweringOutput>`; failures are not cached and cache
namespaces cannot cross manifests.

## CLI

The package includes `monty-lower`:

```bash
monty-lower \
  --manifest monty-v0.0.19.json \
  --input input.py \
  --output output.py \
  --report report.json \
  --deny-needs-review
```

It exits `2` when the deny flag encounters `needs_review` or `not_lowerable`.

## Guarantees

- Rust `unsafe` is forbidden.
- Source byte ranges and arithmetic are checked.
- Rules require matching manifest evidence.
- Final output is reparsed.
- New non-supported features must enter the coverage table.
- Exact-Monty differential fixtures protect observable behavior.

Full documentation, examples, manifests, and benchmark reports:
<https://github.com/fswair/monty-compat>.
