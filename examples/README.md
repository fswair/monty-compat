# Examples

Run examples from the repository root after installing development dependencies:

```bash
uv sync --locked --extra dev
```

## Python

```bash
uv run python examples/python/transpile_and_run.py
uv run python examples/python/error_handling.py
uv run python examples/python/inspect_capabilities.py --monty-root /path/to/monty
```

- `transpile_and_run.py` lowers pattern matching and executes the result through
  exact `pydantic-monty`.
- `error_handling.py` demonstrates fail-closed behavior for unsupported release
  aliases and non-representable generator semantics.
- `inspect_capabilities.py` queries module and nested runtime-type attributes
  from a local checkout or the latest released source.

## Rust

```bash
cargo run --locked -p monty-compat --example transpile
cargo run --locked -p monty-compat-extract --example inspect_release -- 0.0.19
```

The extractor example performs a bounded network release lookup/download. The
transpiler example uses the committed exact manifest and performs no network I/O.

## Benchmarks

See [the benchmark report](../docs/benchmarks.md) for commands and methodology.
