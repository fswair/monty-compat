# Discovery and manifest generation

Discovery produces the evidence that makes lowering release-aware. It is an
offline maintainer workflow and is not invoked by `transpiler(...)`.

## Contents

- [Evidence layers](#evidence-layers)
- [Exact-release pipeline](#exact-release-pipeline)
- [Worker boundaries](#worker-boundaries)
- [Status model](#status-model)
- [Generated corpus and minimization](#generated-corpus-and-minimization)
- [Manifest structure](#manifest-structure)
- [Adding a Monty release](#adding-a-monty-release)
- [Development modes](#development-modes)

## Evidence layers

### Static extraction

The Rust extractor scans Monty's source without executing it and records:

- builtin functions and type constructors;
- exception types;
- importable modules and direct module attributes;
- canonical runtime type paths and nested attributes.

Both local trees and ZIP archives produce the same deterministic graph schema.

### Baseline semantic probes

Small hand-authored programs isolate one statement, expression, protocol, or
runtime seam. Each probe runs on CPython and exact Monty. Results compare values
strictly, including type-sensitive distinctions, or normalize exception
fingerprints.

### Generated acceptance corpus

`pysource-codegen` supplies deterministic grammar combinations that humans may
not anticipate. It broadens parser/type-checker coverage but is not a semantic
oracle.

### Minimization

`pysource-minimize` proposes smaller failing ASTs. Rust accepts a candidate only
when exact Monty returns the same normalized status, exception type, and
message. The Python library proposes; Rust owns the keep/reject verdict.

## Exact-release pipeline

```bash
cargo run --release --manifest-path crates/monty-discover/Cargo.toml -- \
  --release 0.0.19 \
  --seeds 1000 \
  --python .venv/bin/python \
  --output manifests/monty-v0.0.19.json
```

Accepted release spellings are `latest`, `0.0.19`, and `v0.0.19`.

The command:

1. resolves immutable release metadata;
2. validates the URL-safe release tag;
3. downloads and scans the exact archive with bounded native HTTP/ZIP code;
4. compares the resolved runtime version with `LINKED_MONTY_VERSION`;
5. refuses a mismatch before probing;
6. records the CPython oracle fingerprint;
7. runs baseline and generated probes through bounded JSONL workers;
8. minimizes generated failures unless `--no-minimize` is set;
9. writes the manifest through a create-new temporary file, `sync_all`, and
   atomic rename.

Release pipelines default to 1,000 generated seeds when `--release` is present
and `--seeds` is omitted. Pass `--generated-at` with an ISO-8601 timestamp when
reproducible metadata is required.

## Worker boundaries

### Rust owns

- exact Monty execution;
- probe scheduling and timeouts;
- classification and aggregation;
- crash/hang isolation and worker restart;
- release/runtime identity checks;
- minimizer fingerprint verdicts;
- final JSON construction and atomic output.

### Python owns

- the CPython oracle;
- `ast.parse` and `compile` using the actual selected Python grammar;
- deterministic `pysource-codegen` calls;
- optional `pysource-minimize` candidate generation.

This boundary is deliberate. Reimplementing CPython parsing or the Python-only
generators approximately in Rust would change the experiment. The long-lived
Python worker avoids per-probe startup overhead while Rust keeps resource and
semantic control.

`pysource-codegen` and `pysource-minimize` live only in the discovery extra and
worker. They are not dependencies of the public Rust crates and are not used by
the end-user transpilation path.

## Status model

Baseline features use stable classifications:

| Status | Meaning |
|---|---|
| `supported` | Monty and CPython agree for the semantic probe. |
| `unsupported_parse` | Monty rejects the syntax during parsing. |
| `unsupported_type_check` | Parsing succeeds but Monty's type checker rejects it. |
| `unsupported_runtime` | The program reaches runtime and the feature is unavailable. |
| `semantic_mismatch` | Both run, but values/types/behavior differ. |
| `crash` | The isolated Monty worker exits unexpectedly. |
| `timeout` | The worker exceeds its bounded deadline and is killed. |
| `invalid_probe` | The hand-authored probe is invalid for the CPython oracle. |
| `unknown_error` | Failure cannot yet be assigned safely. |

Generated outcomes add `completed`, `generation_error`, and `guard_rejected`.
They remain in a separate report from reviewed semantic features.

## Generated corpus and minimization

Generated code is not trusted for execution:

1. Generate deterministic source from a seed.
2. Enforce source-byte and AST-node bounds.
3. Run `ast.parse` and `compile` on the raw module.
4. Move its entire body under `if False`.
5. Compile the inert form again.
6. Submit only the inert form to Monty.

Monty still parses and type-checks the generated AST combinations, but none of
the generated statements are evaluated.

The report retains:

- seed and generator version;
- raw source and SHA-256;
- AST node kinds/count;
- safety mode and outcome;
- normalized error data;
- minimized source and reduction statistics;
- all seeds mapping to one minimized fingerprint;
- deterministic promotion candidates marked `needs_semantic_probe`.

A generated success does not automatically mark a capability supported. A
generated failure does not automatically define a lowering. Both require a
reviewed atomic semantic probe.

## Manifest structure

Committed manifests use schema version 2:

```json
{
  "schema_version": 2,
  "generated_at": "...",
  "target": {
    "repository": "pydantic/monty",
    "tag": "v0.0.19",
    "runtime_distribution": "pydantic-monty",
    "runtime_version": "0.0.19",
    "published_at": "...",
    "release_url": "...",
    "platform": "...",
    "build_features": []
  },
  "oracle": {
    "implementation": "CPython",
    "version": "..."
  },
  "static_capabilities": {},
  "behavioral_capabilities": {
    "features": {},
    "generated_corpus": {},
    "minimized_failures": {},
    "promotion_candidates": {}
  }
}
```

Lowering consumes only the exact target fingerprint and reviewed feature
statuses, but the full evidence remains committed for auditability.

## Adding a Monty release

1. Change the exact `monty` and `monty-types` versions in
   `crates/monty-discover/Cargo.toml`.
2. Update `LINKED_MONTY_VERSION` through the normal build metadata path.
3. Regenerate the nested `crates/monty-discover/Cargo.lock`.
4. Run the release command with the matching Python worker and at least the
   default 1,000 seeds.
5. Review new baseline failures and minimized promotion candidates.
6. Add reviewed probes for meaningful generated seams.
7. Extend lowering coverage for every newly non-supported feature.
8. Add golden and differential fixtures before implementing new rewrites.
9. Embed the new manifest through the Python binding build registry.
10. Run root and nested cargo-deny audits, all Rust/Python tests, Clippy, Ruff,
    mypy, package dry-runs, and exact-Monty smoke execution.

Do not probe one Monty release and label the result as another. A version
mismatch is a hard error, not a warning override in the Rust release pipeline.

## Development modes

Baseline only:

```bash
cargo run --release --manifest-path crates/monty-discover/Cargo.toml -- \
  --baseline --python .venv/bin/python \
  --output monty-behavioral-capabilities.json
```

Baseline plus a smaller generated development corpus:

```bash
cargo run --release --manifest-path crates/monty-discover/Cargo.toml -- \
  --baseline --seeds 100 \
  --python .venv/bin/python \
  --output monty-behavioral-capabilities.json
```

Bound minimizer checks:

```bash
cargo run --release --manifest-path crates/monty-discover/Cargo.toml -- \
  --seeds 100 --minimizer-max-checks 500 \
  --python .venv/bin/python
```

Acceptance fuzzing without minimization:

```bash
cargo run --release --manifest-path crates/monty-discover/Cargo.toml -- \
  --seeds 100 --no-minimize \
  --python .venv/bin/python
```

CPython versions may generate different grammar nodes. The manifest retains the
oracle fingerprint so these legitimate corpus changes remain attributable.
