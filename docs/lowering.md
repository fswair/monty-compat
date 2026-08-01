# Lowering semantics

Lowering is an evidence-gated source-to-source transformation. Its contract is
stronger than “Monty accepts the output”: for an applied rule, the relevant
observable CPython behavior must be preserved within the rule's proven static
preconditions.

## Contents

- [Decision model](#decision-model)
- [Pipeline](#pipeline)
- [Examples](#examples)
- [Contextual lowering](#contextual-lowering)
- [Diagnostics](#diagnostics)
- [Non-goals](#non-goals)
- [Adding a rule](#adding-a-rule)

## Decision model

Every feature classified as non-supported in the exact release manifest has a
coverage entry:

| Availability | Guarantee |
|---|---|
| `Automatic` | Every occurrence represented by the feature has a safe rewrite. |
| `Contextual` | A safe rewrite exists only when conservative source facts prove all required preconditions. |
| `NotLowerable` | Monty's supported surface cannot represent required observable semantics. |

The engine does not infer support from parser acceptance alone. A lowering rule
runs only when the selected manifest contains matching behavioral evidence.

## Pipeline

For each call:

1. Parse the source with the Ruff parser version pinned to the target Monty
   grammar.
2. Collect conservative facts about statically identifiable classes, methods,
   literals, bindings, and scopes.
3. Visit statements and expressions and correlate each candidate with a stable
   manifest feature ID.
4. Plan checked, non-overlapping UTF-8 byte edits.
5. Apply edits and repeat until no rule emits another edit, up to 128 passes.
6. Inject deduplicated compatibility helpers when required.
7. Parse the final source again.
8. Return code plus diagnostics, or a fallible error.

The Python binding adds one final policy: if any diagnostic is not `Applied`, it
raises `TranspilationError` and returns no code.

## Examples

### Pattern matching

Input:

```python
value = 2
match value:
    case 1:
        result = "one"
    case _:
        result = "other"
result
```

Lowered shape for Monty 0.0.19:

```python
value = 2
_monty_compat_match_subject_0 = (value)
_monty_compat_match_done_1 = False
if not _monty_compat_match_done_1:
    _monty_compat_match_case_2 = False
    if (_monty_compat_match_subject_0) == (1):
        _monty_compat_match_case_2 = True
    if _monty_compat_match_case_2:
        _monty_compat_match_done_1 = True
        result = "one"
if not _monty_compat_match_done_1:
    _monty_compat_match_case_3 = False
    if True:
        _monty_compat_match_case_3 = True
    if _monty_compat_match_case_3:
        _monty_compat_match_done_1 = True
        result = "other"
result
```

The subject is evaluated once and case order/short-circuiting are explicit.
Sequence, mapping, OR, guard, and selected class patterns add rule-specific
checks while retaining the same single-subject discipline.

### Function decorators

Input:

```python
def decorate(fn):
    return lambda: fn() + 1

@decorate
def value():
    return 2
```

Output:

```python
def decorate(fn):
    return lambda: fn() + 1

_monty_compat_decorator_0 = decorate
def value():
    return 2
value = _monty_compat_decorator_0(value)
```

Effectful decorator expressions are captured before the function definition;
multiple decorators are applied bottom-up like CPython.

### Complex `for` targets

Input:

```python
class Item:
    pass

item = Item()
for item.value in [1, 2, 3]:
    pass
result = item.value
```

Output shape:

```python
class Item:
    pass

item = Item()
for _monty_compat_target_0 in [1, 2, 3]:
    item.value = _monty_compat_target_0
    pass
result = item.value
```

Python allows an attribute as a `for` assignment target. Here `item` already
exists; each iteration is equivalent to assigning the next value to
`item.value`. The iterable is not duplicated and target assignment occurs at
the start of each iteration.

### Complex `with` targets

Input:

```python
with Context() as item.value:
    consume(item.value)
```

Output shape:

```python
with Context() as _monty_compat_target_0:
    item.value = _monty_compat_target_0
    consume(item.value)
```

Context-manager entry/exit remains owned by Monty; only the unsupported target
binding form is lowered.

## Contextual lowering

Contextual rules use deliberately narrow proofs. Examples include:

- lowering lazy builtin results only when their construction is statically
  dead and therefore no eager iteration can become observable;
- repairing `asyncio.gather(return_exceptions=True)` only for a supported,
  statically recognized call shape;
- lowering `async with` only when the represented body is statically
  non-raising under the implemented rule;
- hoisting class-body comprehensions only when scope and captured bindings can
  be preserved;
- repairing identity-lambda late binding only for the recognized one-variable
  pattern;
- snapshotting `__exit__` only when the body cannot expose missing exception
  and traceback semantics;
- dispatching user-class protocol calls only when the receiver class and method
  are statically resolved without changing reflected-operation precedence.

If a precondition is unknown, the result is a diagnostic rather than an
optimistic rewrite.

## Diagnostics

Low-level Rust output contains one or more diagnostics:

```json
{
  "rule": "statement.generator",
  "disposition": "not_lowerable",
  "start": 18,
  "end": 25,
  "message": "generator suspension cannot be represented by this Monty release"
}
```

Use the CLI report for audit pipelines:

```bash
monty-lower \
  --manifest manifests/monty-v0.0.19.json \
  --input input.py \
  --output output.py \
  --report report.json \
  --deny-needs-review
```

With `--deny-needs-review`, exit status `2` means the report contains a seam the
engine did not prove safe.

## Non-goals

The engine deliberately refuses transformations requiring semantics Monty does
not expose, including:

- generator suspension and generator identity/type behavior;
- `async for` state-machine semantics;
- exception inheritance or Monty-missing exception classes;
- traceback, exception cause, and exception-group construction;
- runtime mutation of `__class__` or Python type identity;
- reflected binary dispatch ordering that cannot be reproduced;
- CPython validation of protocol return types when the target lacks it;
- deletion semantics unavailable in the target runtime.

No helper class or replacement exception is invented merely to make source
parse. Unsupported stays unsupported until the target feature set can express
the behavior.

## Adding a rule

1. Add or identify a minimal semantic probe with a stable feature ID.
2. Run it on CPython and the exact Monty runtime and commit the evidence.
3. Decide whether the seam is automatic, contextual, or not lowerable.
4. Implement only the facts required for a conservative proof.
5. Emit a diagnostic for every candidate the rule encounters.
6. Add a golden source fixture and a differential fixture.
7. Confirm original CPython, lowered CPython, and lowered exact-Monty envelopes
   agree, including stdout/stderr.
8. Update `LOWERING_COVERAGE`; CI must fail if any manifest seam is missing.
9. Run malformed/Unicode safety tests and Clippy with warnings denied.

Generated-corpus failures may suggest a rule but cannot replace the reviewed
semantic probe in step 1.
