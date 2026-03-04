# monty-compat

AST-based Python compatibility checker for the [Monty](https://github.com/pydantic/monty) sandbox.

Parses the Monty Rust source to extract every implemented builtin function, type constructor, exception type, and stdlib module, then checks arbitrary Python code for unsupported features — **without executing it**.

## Installation

```bash
pip install monty-compat
```

Or for development (from checkout):

```bash
pip install -e .
```

## Quick start

```python
from monty_compat import monty_compat

# One-shot check — loads capabilities from cache (or builds on first run)
ok, reasons = monty_compat("x = [i * 2 for i in range(10)]")
# ok=True, reasons=[]

ok, reasons = monty_compat("import json; json.loads('{}')")
# ok=False, reasons=["module 'json' is not supported by Monty"]

# Force rebuild (ignores cache)
ok, reasons = monty_compat(code, cache='regenerate')

# Skip cache entirely, read from local monty checkout
ok, reasons = monty_compat(code, cache='off', monty_root='/path/to/monty')
```

## Cache

Capabilities are cached at `~/.monty_compat/monty_{version}_compat.json`.

- Default TTL: **12 hours**
- Cache is version-keyed — each installed `pydantic-monty` version gets its own file
- `cache='regenerate'` forces a rebuild and overwrites the cache
- `cache='off'` skips all cache I/O

## Lower-level API

```python
from monty_compat import MontyCapabilities

# Build from GitHub (downloads ZIP in memory)
caps = MontyCapabilities.from_github()

# Build from a local monty repo checkout
caps = MontyCapabilities.from_local('/path/to/monty')

# Inspect capabilities
caps.builtin_functions    # frozenset: abs, all, any, bin, chr, …
caps.type_constructors    # frozenset: int, str, list, dict, …
caps.exception_types      # frozenset: ValueError, TypeError, …
caps.modules              # frozenset: sys, typing, asyncio, pathlib, os, re, …
caps.module_attributes    # dict:      {'asyncio': {'gather', 'run'}, …}

# Check code
ok, reasons = caps.check_code(some_code)

# Pretty-print
print(caps.summary())
```

## What is checked

| Pattern | Example | Check |
|---------|---------|-------|
| `import X` | `import json` | Is `json` a supported module? |
| `from X import Y` | `from asyncio import subprocess` | Is `Y` in `X`'s known attributes? |
| Builtin calls | `eval(...)` | Is the builtin implemented in Monty? |

## Supported modules (as of 2026-03)

| Module | Available attributes |
|--------|---------------------|
| `re` | `search`, `match`, `sub`, `findall`, … |
| `asyncio` | `gather`, `run` |
| `os` | `getenv`, `environ` |
| `pathlib` | `Path` |
| `sys` | `platform`, `version`, `version_info`, `stdout`, `stderr` |
| `typing` | `Any`, `Optional`, `Union`, `List`, `Dict`, `Callable`, … |
