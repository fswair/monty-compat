---
hide:
  - path
  - toc
---

<div class="hero">
  <h1>Python, shaped for Monty.</h1>
  <p><code>monty-compat</code> turns unsupported Python into evidence-backed
  source that Monty can run—and refuses the transformation when Python
  semantics cannot be kept.</p>
  <p class="hero-actions">
    <a class="md-button md-button--primary" href="python-api/">Get started</a>
    <a class="md-button" href="lowering/">See lowering examples</a>
  </p>
</div>

<div class="release-rail">
  <div>
    <span>default · offline</span>
    <strong>verified</strong>
    <p>The newest manifest compiled into your wheel. Stable for its lifetime.</p>
  </div>
  <div>
    <span>opt-in · network</span>
    <strong>latest</strong>
    <p>A bounded, hash-checked channel that must match your lowering engine.</p>
  </div>
</div>

## One function at runtime

```python
from monty_compat import transpiler
from pydantic_monty import Monty

source = """
value = 2
match value:
    case 2:
        result = f"ok:{value}"
result
"""

with Monty() as pool:
    with pool.checkout() as session:
        result = session.feed_run(transpiler(source))
```

The default path is native Rust, release-pinned, bounded, and offline. It does
not execute your input, start discovery workers, or take ownership of Monty's
runtime API.

## Evidence before rewriting

1. **Extract** the real builtin, module, exception, and runtime-type surface.
2. **Probe** one exact Monty release against CPython.
3. **Lower** only seams whose observable behavior can be preserved.

Unsupported inheritance, generator suspension, traceback objects, exception
groups, and runtime dispatch tricks remain explicit errors—not approximations.

## Choose your path

| You want to… | Start here |
|---|---|
| Transpile from Python | [Python API](python-api.md) |
| Understand `verified` and `latest` | [Release selection](releases.md) |
| See before/after transformations | [Lowering](lowering.md) |
| Generate a manifest | [Discovery](discovery.md) |
| Embed the Rust engine | [Rust API](rust-api.md) |
| Reproduce latency measurements | [Benchmarks](benchmarks.md) |
