# Release selection

The `release` argument separates reproducibility from freshness.

## `verified`: the default

```python
from monty_compat import transpiler

lowered = transpiler(source)
lowered = transpiler(source, release="verified")
```

`verified` uses the newest manifest compiled into the installed wheel. In
monty-compat 0.5.0 it resolves to Monty 0.0.19. It is deterministic, requires no
network access, and never changes during the lifetime of that wheel.

Use it for production workloads, offline environments, tests, and reproducible
builds.

## `latest`: explicit freshness

```python
lowered = transpiler(source, release="latest")
```

`latest` downloads the small manifest channel published with these docs, then
downloads its selected behavioral manifest. It accepts the result only when:

- both HTTP responses remain within fixed byte limits;
- the channel schema is supported;
- the manifest URL uses HTTPS;
- the manifest SHA-256 matches the channel;
- the manifest's Monty target matches the channel release;
- the channel declares the installed monty-compat engine compatible;
- every non-supported feature is known to that lowering engine.

The first successful resolution is cached for the current process. A new
process resolves the channel again. Any network or validation failure raises
`TranspilationError`; `latest` never silently falls back to `verified`.

The channel contains reviewed monty-compat manifests, not an unprobed upstream
Monty version.

## Exact releases

```python
transpiler(source, release="0.0.19")
transpiler(source, release="v0.0.19")
```

Exact releases must be bundled in the installed wheel. They are offline and
fail explicitly when unavailable.

| Mode | Network | Mutable during a process | Intended use |
|---|---:|---:|---|
| `verified` | no | no | production default |
| `latest` | first successful resolution | no | opt-in freshness |
| exact version | no | no | pinned runtime |
