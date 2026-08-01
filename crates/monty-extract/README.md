# monty-compat-extract

Static capability extraction from an exact
[Monty](https://github.com/pydantic/monty) source tree or release archive.

It discovers builtins, exception types, modules, module attributes, runtime
types, and nested attributes such as `pathlib.Path.is_dir` without executing
Monty source.

## Usage

```toml
[dependencies]
monty-compat-extract = "0.5.0"
```

Extract a published release:

```rust
use monty_compat_extract::{extract_release, resolve_release};

let release = resolve_release("0.0.19")?;
let graph = extract_release(&release)?;
assert!(graph.modules.contains("pathlib"));
assert!(graph
    .type_attributes
    .get("pathlib.Path")
    .is_some_and(|attributes| attributes.contains("is_dir")));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Extract local or in-memory source:

```rust
let local = monty_compat_extract::extract_local("/path/to/monty")?;
let archive = std::fs::read("monty-v0.0.19.zip")?;
let zipped = monty_compat_extract::extract_zip(&archive)?;
assert_eq!(local, zipped);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`CapabilityGraph` uses sorted maps/sets so `to_json_pretty` is deterministic.

## Input safety

- bounded compressed downloads and response metadata;
- bounded ZIP entry count, individual files, and expanded total;
- validated archive paths and UTF-8;
- portable Rustls TLS on Linux and native platform TLS elsewhere, with bounded
  redirects/timeouts;
- in-memory archive scanning, never filesystem extraction;
- Rust `unsafe` forbidden.

## CLI

The package includes `monty-extract`:

```bash
monty-extract --root /path/to/monty --output capabilities.json
monty-extract --archive monty-v0.0.19.zip --output capabilities.json
```

Exactly one source is required. JSON is written to stdout when `--output` is
omitted.

Full API documentation, discovery architecture, examples, and benchmark
reports: <https://github.com/fswair/monty-compat>.
