"""Atomic baseline probes for Python syntax and class-like protocols."""

from __future__ import annotations

from .probe_catalog_extended import EXTENDED_PROBES
from .probe_catalog_protocols import PROTOCOL_MATRIX_PROBES
from .probe_catalog_semantics import SEMANTIC_PROBES
from .probes import ProbeSpec

BASELINE_PROBES: tuple[ProbeSpec, ...] = (
    (
        ProbeSpec("statement.assign", "statement", "x = 3\nx", "Simple assignment"),
        ProbeSpec(
            "statement.annotated_assign", "statement", "x: int = 3\nx", "Annotated assignment"
        ),
        ProbeSpec(
            "statement.augmented_assign", "statement", "x = 2\nx += 3\nx", "Augmented assignment"
        ),
        ProbeSpec(
            "statement.delete", "statement", "x = {'a': 1}\ndel x['a']\nx", "Delete statement"
        ),
        ProbeSpec(
            "statement.if_else",
            "statement",
            "x = 4\n'yes' if x > 2 else 'no'",
            "Conditional branching",
        ),
        ProbeSpec(
            "statement.for",
            "statement",
            "total = 0\nfor x in [1, 2, 3]:\n    total += x\ntotal",
            "For loop",
        ),
        ProbeSpec(
            "statement.while", "statement", "x = 0\nwhile x < 3:\n    x += 1\nx", "While loop"
        ),
        ProbeSpec(
            "statement.break",
            "statement",
            "x = 0\nwhile True:\n    x += 1\n    if x == 2:\n        break\nx",
            "Break statement",
        ),
        ProbeSpec(
            "statement.continue",
            "statement",
            "out = []\nfor x in range(4):\n    if x == 2:\n"
            "        continue\n    out.append(x)\nout",
            "Continue statement",
        ),
        ProbeSpec("statement.pass", "statement", "if True:\n    pass\nTrue", "Pass statement"),
        ProbeSpec(
            "statement.for_else",
            "statement",
            "result = []\nfor x in [1, 2]:\n    result.append(x)\n"
            "else:\n    result.append(3)\nresult",
            "For-else statement",
        ),
        ProbeSpec(
            "statement.while_else",
            "statement",
            "x = 0\nwhile x < 2:\n    x += 1\nelse:\n    x += 3\nx",
            "While-else statement",
        ),
        ProbeSpec(
            "statement.try_except",
            "statement",
            "try:\n    1 / 0\nexcept ZeroDivisionError:\n    result = 'caught'\nresult",
            "Exception handling",
        ),
        ProbeSpec(
            "statement.try_finally",
            "statement",
            "out = []\ntry:\n    out.append('try')\nfinally:\n    out.append('finally')\nout",
            "Finally block",
        ),
        ProbeSpec(
            "statement.try_else",
            "statement",
            "try:\n    value = 2\nexcept ValueError:\n    value = 0\nelse:\n    value += 3\nvalue",
            "Try-else statement",
        ),
        ProbeSpec(
            "statement.raise",
            "statement",
            "try:\n    raise ValueError('x')\nexcept ValueError as e:\n    result = str(e)\nresult",
            "Raise statement",
        ),
        ProbeSpec(
            "statement.assert", "statement", "x = 3\nassert x == 3\nTrue", "Passing assertion"
        ),
        ProbeSpec(
            "statement.function",
            "statement",
            "def add(a, b=1):\n    return a + b\nadd(2, 3)",
            "Function definition and return",
        ),
        ProbeSpec(
            "statement.function_decorator",
            "statement",
            "def decorate(fn):\n    return lambda: fn() + 1\n"
            "@decorate\ndef value():\n    return 2\nvalue()",
            "Function decorator",
        ),
        ProbeSpec(
            "statement.async_function",
            "async",
            "import asyncio\nasync def value():\n    return 3\nasyncio.run(value())",
            "Async function definition and execution",
        ),
        ProbeSpec(
            "statement.global",
            "scope",
            "value = 1\ndef change():\n    global value\n    value = 4\nchange()\nvalue",
            "Global binding",
        ),
        ProbeSpec(
            "statement.nonlocal",
            "scope",
            "def outer():\n    x = 1\n    def inner():\n        nonlocal x\n"
            "        x = 5\n    inner()\n    return x\nouter()",
            "Nonlocal closure binding",
        ),
        ProbeSpec(
            "expression.lambda",
            "expression",
            "add = lambda a, b: a + b\nadd(2, 3)",
            "Lambda expression",
        ),
        ProbeSpec(
            "expression.walrus",
            "expression",
            "values = [1, 2, 3]\n(n := len(values), n)",
            "Assignment expression",
        ),
        ProbeSpec(
            "expression.await",
            "async",
            "import asyncio\nasync def child():\n    return 3\n"
            "async def parent():\n    return await child()\nasyncio.run(parent())",
            "Await expression",
        ),
        ProbeSpec(
            "expression.unpack_assignment",
            "expression",
            "first, *middle, last = [1, 2, 3, 4]\n(first, middle, last)",
            "Starred assignment unpacking",
        ),
        ProbeSpec(
            "expression.fstring.basic",
            "fstring",
            "name = 'Ada'\nf'hello {name}'",
            "Basic f-string interpolation",
        ),
        ProbeSpec(
            "expression.fstring.repr",
            "fstring",
            "value = 'x'\nf'{value!r}'",
            "F-string repr conversion",
        ),
        ProbeSpec(
            "expression.fstring.format_spec",
            "fstring",
            "value = 7\nf'{value:04d}'",
            "F-string format specification",
        ),
        ProbeSpec(
            "expression.fstring.debug", "fstring", "value = 7\nf'{value=}'", "F-string debug syntax"
        ),
        ProbeSpec(
            "comprehension.list", "comprehension", "[x * 2 for x in range(4)]", "List comprehension"
        ),
        ProbeSpec(
            "comprehension.set",
            "comprehension",
            "{x % 2 for x in range(4)} == {0, 1}",
            "Set comprehension",
        ),
        ProbeSpec(
            "comprehension.dict",
            "comprehension",
            "{x: x * 2 for x in range(3)}",
            "Dictionary comprehension",
        ),
        ProbeSpec(
            "comprehension.generator",
            "comprehension",
            "list(x * 2 for x in range(3))",
            "Generator expression",
        ),
        ProbeSpec(
            "comprehension.filter",
            "comprehension",
            "[x for x in range(6) if x % 2 == 0]",
            "Filtered comprehension",
        ),
        ProbeSpec(
            "statement.generator",
            "generator",
            "def values():\n    yield 1\n    yield 2\nlist(values())",
            "Generator function",
        ),
        ProbeSpec(
            "statement.yield_from",
            "generator",
            "def values():\n    yield from [1, 2]\nlist(values())",
            "Yield-from expression",
        ),
        ProbeSpec(
            "class.basic",
            "class",
            "class Point:\n    def __init__(self, x, y):\n        self.x = x\n"
            "        self.y = y\n    def total(self):\n        return self.x + self.y\n"
            "Point(2, 3).total()",
            "Class, initializer, attributes, and bound method",
        ),
        ProbeSpec(
            "class.class_attribute",
            "class",
            "class Counter:\n    value = 3\nCounter.value",
            "Class attribute lookup",
        ),
        ProbeSpec(
            "class.inheritance",
            "class",
            "class Base:\n    def value(self):\n        return 2\n"
            "class Child(Base):\n    pass\nChild().value()",
            "Single inheritance",
        ),
        ProbeSpec(
            "class.super",
            "class",
            "class Base:\n    def value(self):\n        return 2\nclass Child(Base):\n"
            "    def value(self):\n        return super().value() + 1\nChild().value()",
            "Zero-argument super",
        ),
        ProbeSpec(
            "class.decorator",
            "class",
            "def decorate(cls):\n    cls.answer = 42\n    return cls\n"
            "@decorate\nclass Example:\n    pass\nExample.answer",
            "Class decorator",
        ),
        ProbeSpec(
            "dataclass.basic",
            "dataclass",
            "from dataclasses import dataclass\n@dataclass\nclass Point:\n"
            "    x: int\n    y: int = 2\np = Point(3)\n(p.x, p.y)",
            "Basic dataclass fields and default",
        ),
        ProbeSpec(
            "protocol.iterator",
            "protocol",
            "class Count:\n    def __init__(self):\n        self.x = 0\n"
            "    def __iter__(self):\n        return self\n    def __next__(self):\n"
            "        if self.x == 3:\n            raise StopIteration\n"
            "        value = self.x\n        self.x += 1\n        return value\nlist(Count())",
            "User-defined iterator protocol",
        ),
        ProbeSpec(
            "protocol.context_manager",
            "protocol",
            "events = []\nclass Context:\n    def __enter__(self):\n"
            "        events.append('enter')\n        return 3\n"
            "    def __exit__(self, exc_type, exc, tb):\n        events.append('exit')\n"
            "with Context() as value:\n    events.append(value)\nevents",
            "Context manager protocol",
        ),
        ProbeSpec(
            "match.literal",
            "match",
            "value = 2\nmatch value:\n    case 1:\n        result = 'one'\n"
            "    case 2:\n        result = 'two'\n    case _:\n        result = 'other'\nresult",
            "Literal match pattern",
        ),
        ProbeSpec(
            "match.or",
            "match",
            "value = 2\nmatch value:\n    case 1 | 2:\n        result = True\n"
            "    case _:\n        result = False\nresult",
            "OR match pattern",
        ),
        ProbeSpec(
            "match.sequence",
            "match",
            "value = [1, 2]\nmatch value:\n    case [a, b]:\n        result = a + b\n"
            "    case _:\n        result = 0\nresult",
            "Sequence match pattern",
        ),
        ProbeSpec(
            "match.mapping",
            "match",
            "value = {'x': 3}\nmatch value:\n    case {'x': x}:\n        result = x\n"
            "    case _:\n        result = 0\nresult",
            "Mapping match pattern",
        ),
        ProbeSpec(
            "match.guard",
            "match",
            "value = 3\nmatch value:\n    case x if x > 2:\n        result = 'large'\n"
            "    case _:\n        result = 'small'\nresult",
            "Guarded match pattern",
        ),
        ProbeSpec(
            "match.class",
            "match",
            "class Point:\n    __match_args__ = ('x', 'y')\n    def __init__(self, x, y):\n"
            "        self.x = x\n        self.y = y\nvalue = Point(2, 3)\nmatch value:\n"
            "    case Point(x, y):\n        result = x + y\n"
            "    case _:\n        result = 0\nresult",
            "Class match pattern",
        ),
        ProbeSpec(
            "expression.starred_unpack",
            "expression",
            "values = [2, 3]\n[1, *values, 4]",
            "Starred iterable unpacking",
        ),
        ProbeSpec("expression.chained_compare", "expression", "1 < 2 < 3", "Chained comparison"),
        ProbeSpec("import.module", "import", "import math\nmath.sqrt(9)", "Module import"),
        ProbeSpec(
            "import.from",
            "import",
            "from math import sqrt\nsqrt(9)",
            "From import",
        ),
    )
    + EXTENDED_PROBES
    + SEMANTIC_PROBES
    + PROTOCOL_MATRIX_PROBES
)
