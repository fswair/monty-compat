# Python API

This reference covers the public API exported by `monty_compat`. The native
extension is an implementation detail; import public names from
`monty_compat`, not `monty_compat._native`.

## Contents

- [Installation](#installation)
- [`transpiler`](#transpiler)
- [`TranspilationError`](#transpilationerror)
- [`MontyCapabilities`](#montycapabilities)
- [Discovery helpers](#discovery-helpers)
- [Choosing the right API](#choosing-the-right-api)

## Installation

```bash
pip install monty-compat
```

Behavioral discovery is optional and deliberately outside the runtime hot
path:

```bash
pip install 'monty-compat[discovery]'
```

The discovery extra installs `pydantic-monty`, `pysource-codegen`, and
`pysource-minimize`. A normal installation does not install or run those
packages.

## `transpiler`

```python
def transpiler(
    code: str,
    release: str | Literal["verified", "latest"] = "verified",
) -> str: ...
```

Transpile Python source for one exact bundled Monty release.

### Parameters

| Parameter | Type | Default | Meaning |
|---|---|---|---|
| `code` | `str` | required | Complete Python module source. |
| `release` | `str \| Literal["verified", "latest"]` | `"verified"` | Offline wheel-pinned manifest, opt-in remote channel, or exact bundled version. |

`verified` resolves to the newest manifest compiled into the installed wheel.
For monty-compat 0.5.0 this is Monty 0.0.19. It performs no network access and
is the reproducible default.

`latest` explicitly queries
`https://fswair.github.io/monty-compat/manifest-channel.json`. The channel and
manifest are size-bounded; the manifest must match the advertised SHA-256,
target release, current engine compatibility list, and known non-supported
feature coverage. The validated transpiler is cached for the process. Network,
validation, and compatibility failures raise `TranspilationError` without
falling back to `verified`.

Bare versions such as `0.0.19` and tags such as `v0.0.19` select an exact
bundled manifest and remain offline. An unknown release never falls back.

### Return value

The return value is ordinary Python source:

- already supported source is returned unchanged;
- proven-safe unsupported seams are lowered;
- generated compatibility helper names use the reserved
  `_monty_compat_...` prefix;
- the returned source is parsed again before it leaves Rust.

The function does not execute either the input or output.

```python
from monty_compat import transpiler

source = """
value = 2
match value:
    case 2:
        result = f"ok:{value}"
    case _:
        result = "other"
result
"""

lowered = transpiler(source, release="0.0.19")
assert "match value:" not in lowered
```

### Execution with Monty

`monty-compat` returns code and does not own Monty's API:

```python
from monty_compat import transpiler
from pydantic_monty import Monty

with Monty() as pool:
    with pool.checkout() as session:
        result = session.feed_run(transpiler(source, "0.0.19"))
```

### Cache and concurrency

The Python binding maintains one process-global Rust `Transpiler` per bundled
release. Each instance has a bounded exact-source cache:

- default maximum: 256 successful entries and approximately 32 MiB;
- the key is the complete source string inside a release-specific namespace;
- only successful outputs are cached;
- exact hits reuse the shared Rust artifact;
- failures are recomputed and never retained;
- the GIL is released while parsing and lowering.

The Python function intentionally does not expose cache tuning. Applications
that need explicit bounds or statistics should use the Rust `Transpiler` API.

The `latest` channel is resolved once after the first successful call in a
process. Restart the process to re-resolve the channel.

### Semantic safety

The binding returns code only when every emitted diagnostic is `applied`.
`needs_review` and `not_lowerable` diagnostics become `TranspilationError`.
This makes the convenient API fail closed.

## `TranspilationError`

```python
class TranspilationError(Exception): ...
```

Raised for:

- invalid or unparseable Python source;
- an unknown/unbundled release;
- an invalid bundled manifest;
- internal lowering/edit failure;
- any encountered seam whose semantics cannot be preserved.

The message includes the rule, disposition, source byte range, and explanation
for unsafe diagnostics:

```python
from monty_compat import TranspilationError, transpiler

try:
    transpiler("def values():\n    yield 1\n")
except TranspilationError as exc:
    print(str(exc))
```

Do not catch this exception and execute the original source as an automatic
fallback; that would bypass the compatibility decision.

## `MontyCapabilities`

`MontyCapabilities` is an immutable dataclass representing the static graph.

### Fields

| Field | Type | Contents |
|---|---|---|
| `builtin_functions` | `frozenset[str]` | Implemented builtin function names. |
| `type_constructors` | `frozenset[str]` | Builtin type constructor names. |
| `exception_types` | `frozenset[str]` | Implemented exception class names. |
| `modules` | `frozenset[str]` | Importable module names. |
| `module_attributes` | `dict[str, frozenset[str]]` | Direct module exports. |
| `type_attributes` | `dict[str, frozenset[str]]` | Attributes keyed by canonical type path. |

Canonical type paths are bare for builtins (`str`, `dict`) and qualified for
imported types (`pathlib.Path`, `datetime.datetime`, `re.Pattern`).

### Constructors

```python
caps = MontyCapabilities.from_local("/path/to/monty")
caps = MontyCapabilities.from_github()  # latest released source
caps = MontyCapabilities.from_github(only_released=False)  # main branch
caps = MontyCapabilities.from_dict(payload)
```

`from_local` and `from_github` use the Rust extractor through a private PyO3
bridge. GitHub archive scanning occurs in memory.

### Queries

```python
caps.get_attributes("pathlib")
caps.get_attributes("pathlib.Path")
caps.supports_path("pathlib.Path.is_dir")
```

These methods query canonical graph paths only. `MontyCapabilities` does not
classify complete source code; use `transpiler` for that fail-closed decision.

Class-level convenience methods use the disk cache:

```python
MontyCapabilities.get_modules()
MontyCapabilities.get_builtins()
MontyCapabilities.get_types()
MontyCapabilities.get_exception_types()
MontyCapabilities.get_attrs_of_module("pathlib")
MontyCapabilities.get_attrs_of_type("pathlib.Path")
```

Pass `cache=False` to force regeneration and `only_released=False` to inspect
main.

### Serialization and presentation

```python
payload = caps.to_dict()                # deterministic JSON-safe values
same = MontyCapabilities.from_dict(payload)
human = caps.summary()                  # readable complete listing
prompt = caps.to_prompt_context()       # structured LLM prompt context
```

`to_dict()` sorts every set and mapping so repeated extraction of the same
source produces stable JSON.

## Discovery helpers

The following exports are maintenance APIs rather than hot-path runtime APIs:

```python
from monty_compat import (
    GeneratedProbeConfig,
    discover_latest_release,
    write_manifest,
)
```

### `GeneratedProbeConfig`

| Field | Default | Meaning |
|---|---:|---|
| `seed_start` | `0` | First deterministic generator seed. |
| `seed_count` | `100` | Number of generated modules. |
| `node_limit` | `100` | Generator node budget. |
| `depth_limit` | `5` | Generator depth budget. |
| `max_source_bytes` | `100_000` | Raw source safety bound. |
| `max_ast_nodes` | `2_000` | Parsed AST safety bound. |

### `discover_latest_release`

Downloads the latest released source, verifies the installed
`pydantic-monty` version by default, runs baseline probes, and optionally adds
an inert generated corpus:

```python
config = GeneratedProbeConfig(seed_count=100)
manifest = discover_latest_release(generated_config=config)
```

Prefer the Rust exact-release pipeline documented in
[Discovery and manifest generation](discovery.md) for producing committed
release manifests. It hard-links the exact Monty version and owns timeouts,
worker restart, minimization verdicts, and atomic output.

### `write_manifest`

```python
path = write_manifest(manifest, "manifest.json")
```

Creates parent directories and writes sorted, indented UTF-8 JSON.

## Choosing the right API

| Goal | API |
|---|---|
| Produce Monty-runnable source | `transpiler` |
| Execute the result | Monty's own `Monty` API |
| Inspect the complete static graph | `MontyCapabilities` |
| Generate a release manifest | Rust `monty-discover` pipeline |
| Tune in-memory lowering cache | Rust `Transpiler` |
