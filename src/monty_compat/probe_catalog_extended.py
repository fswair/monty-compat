"""Extended probes covering lowering primitives and Python AST variants."""

from __future__ import annotations

from textwrap import dedent

from .probes import ProbeSpec


def _probe(id: str, category: str, source: str, description: str) -> ProbeSpec:
    return ProbeSpec(id, category, dedent(source).strip(), description)


EXTENDED_PROBES: tuple[ProbeSpec, ...] = (
    _probe(
        "statement.if",
        "statement",
        """
        value = 3
        if value > 2:
            result = "large"
        else:
            result = "small"
        result
        """,
        "If statement",
    ),
    _probe(
        "statement.chained_assign",
        "statement",
        """
        first = second = 3
        (first, second)
        """,
        "Chained assignment",
    ),
    _probe(
        "statement.destructuring_assign",
        "statement",
        """
        first, second = (2, 3)
        first + second
        """,
        "Destructuring assignment",
    ),
    _probe(
        "statement.delete_name",
        "statement",
        """
        value = 1
        del value
        "value" in globals()
        """,
        "Delete a local name",
    ),
    _probe(
        "statement.delete_attribute",
        "statement",
        """
        class Item:
            pass
        item = Item()
        item.value = 1
        del item.value
        hasattr(item, "value")
        """,
        "Delete an instance attribute",
    ),
    _probe(
        "statement.try_multiple_handlers",
        "statement",
        """
        try:
            int("x")
        except ValueError:
            result = "value"
        except TypeError:
            result = "type"
        result
        """,
        "Multiple except handlers",
    ),
    _probe(
        "statement.try_star",
        "statement",
        """
        result = []
        try:
            raise ValueError("x")
        except* ValueError:
            result.append("caught")
        result
        """,
        "Exception-group except-star statement",
    ),
    _probe(
        "statement.raise_from",
        "statement",
        """
        try:
            try:
                raise ValueError("inner")
            except ValueError as error:
                raise TypeError("outer") from error
        except TypeError as error:
            result = str(error.__cause__)
        result
        """,
        "Explicit exception chaining",
    ),
    _probe(
        "statement.with_multiple",
        "protocol",
        """
        events = []
        class Context:
            def __init__(self, name):
                self.name = name
            def __enter__(self):
                events.append("enter-" + self.name)
                return self.name
            def __exit__(self, exc_type, exc, tb):
                events.append("exit-" + self.name)
        with Context("a") as first, Context("b") as second:
            events.append(first + second)
        events
        """,
        "Multiple context managers",
    ),
    _probe(
        "import.module_alias",
        "import",
        """
        import math as mathematics
        mathematics.sqrt(16)
        """,
        "Aliased module import",
    ),
    _probe(
        "import.from_alias",
        "import",
        """
        from math import sqrt as root
        root(16)
        """,
        "Aliased from import",
    ),
    _probe(
        "function.positional_only",
        "function",
        """
        def subtract(left, /, right):
            return left - right
        subtract(5, right=2)
        """,
        "Positional-only parameter",
    ),
    _probe(
        "function.keyword_only",
        "function",
        """
        def scale(value, *, factor=2):
            return value * factor
        scale(3, factor=4)
        """,
        "Keyword-only parameter",
    ),
    _probe(
        "function.varargs",
        "function",
        """
        def total(*values):
            return sum(values)
        total(1, 2, 3)
        """,
        "Variadic positional parameters",
    ),
    _probe(
        "function.kwargs",
        "function",
        """
        def select(**values):
            return values["answer"]
        select(answer=42)
        """,
        "Variadic keyword parameters",
    ),
    _probe(
        "function.call_star_args",
        "function",
        """
        def add(first, second, third):
            return first + second + third
        values = [1, 2, 3]
        add(*values)
        """,
        "Starred call arguments",
    ),
    _probe(
        "function.call_star_kwargs",
        "function",
        """
        def add(first, second):
            return first + second
        values = {"first": 2, "second": 3}
        add(**values)
        """,
        "Double-starred call keyword arguments",
    ),
    _probe(
        "function.recursion",
        "function",
        """
        def factorial(value):
            if value <= 1:
                return 1
            return value * factorial(value - 1)
        factorial(5)
        """,
        "Recursive function",
    ),
    _probe(
        "function.multilevel_closure",
        "scope",
        """
        def outer(value):
            def middle(offset):
                def inner():
                    return value + offset
                return inner
            return middle
        outer(2)(3)()
        """,
        "Multi-level closure capture",
    ),
    _probe(
        "function.annotations",
        "function",
        """
        def add(left: int, right: int) -> int:
            return left + right
        add(2, 3)
        """,
        "Function annotations",
    ),
    _probe(
        "operator.bool_and_short_circuit",
        "operator",
        """
        calls = []
        def visit():
            calls.append(1)
            return True
        False and visit()
        calls
        """,
        "Short-circuit boolean and",
    ),
    _probe(
        "operator.bool_or_short_circuit",
        "operator",
        """
        calls = []
        def visit():
            calls.append(1)
            return False
        True or visit()
        calls
        """,
        "Short-circuit boolean or",
    ),
    ProbeSpec("operator.add", "operator", "7 + 3", "Addition operator"),
    ProbeSpec("operator.subtract", "operator", "7 - 3", "Subtraction operator"),
    ProbeSpec("operator.multiply", "operator", "7 * 3", "Multiplication operator"),
    ProbeSpec("operator.true_divide", "operator", "7 / 2", "True division operator"),
    ProbeSpec("operator.floor_divide", "operator", "7 // 2", "Floor division operator"),
    ProbeSpec("operator.modulo", "operator", "7 % 3", "Modulo operator"),
    ProbeSpec("operator.power", "operator", "2 ** 8", "Power operator"),
    ProbeSpec("operator.bit_and", "operator", "6 & 3", "Bitwise and operator"),
    ProbeSpec("operator.bit_or", "operator", "4 | 3", "Bitwise or operator"),
    ProbeSpec("operator.bit_xor", "operator", "6 ^ 3", "Bitwise xor operator"),
    ProbeSpec("operator.left_shift", "operator", "3 << 2", "Left shift operator"),
    ProbeSpec("operator.right_shift", "operator", "12 >> 2", "Right shift operator"),
    ProbeSpec("operator.unary_positive", "operator", "+3", "Unary positive operator"),
    ProbeSpec("operator.unary_negative", "operator", "-3", "Unary negative operator"),
    ProbeSpec("operator.unary_invert", "operator", "~3", "Bitwise invert operator"),
    ProbeSpec("operator.unary_not", "operator", "not False", "Boolean not operator"),
    ProbeSpec(
        "operator.identity", "operator", "value = None\nvalue is None", "Identity comparison"
    ),
    ProbeSpec(
        "operator.not_identity",
        "operator",
        "value = []\nvalue is not None",
        "Negative identity comparison",
    ),
    ProbeSpec("operator.membership", "operator", "2 in [1, 2, 3]", "Membership comparison"),
    ProbeSpec(
        "operator.not_membership",
        "operator",
        "4 not in [1, 2, 3]",
        "Negative membership comparison",
    ),
    _probe(
        "operator.evaluation_order",
        "operator",
        """
        events = []
        def value(item):
            events.append(item)
            return item
        value(1) + value(2) * value(3)
        events
        """,
        "Left-to-right operand evaluation",
    ),
    ProbeSpec("expression.bytes_literal", "expression", "b'abc'", "Bytes literal"),
    ProbeSpec("expression.large_integer", "expression", "10 ** 30", "Arbitrary-size integer"),
    ProbeSpec("expression.ellipsis", "expression", "repr(Ellipsis)", "Ellipsis literal"),
    _probe(
        "expression.slice_basic",
        "expression",
        """
        values = [0, 1, 2, 3, 4]
        values[1:4]
        """,
        "Basic slice",
    ),
    _probe(
        "expression.slice_step",
        "expression",
        """
        values = [0, 1, 2, 3, 4]
        values[::-2]
        """,
        "Slice with negative step",
    ),
    _probe(
        "expression.dict_unpack",
        "expression",
        """
        first = {"a": 1}
        second = {"b": 2}
        {**first, **second}
        """,
        "Dictionary unpacking",
    ),
    _probe(
        "expression.literal_containers",
        "expression",
        """
        ([1, 2], (3, 4), {5, 6} == {6, 5}, {"x": 7})
        """,
        "List, tuple, set, and dictionary literals",
    ),
    _probe(
        "comprehension.nested",
        "comprehension",
        """
        [(left, right) for left in [1, 2] for right in [3, 4]]
        """,
        "Nested list comprehension",
    ),
    _probe(
        "comprehension.scope",
        "comprehension",
        """
        value = 10
        result = [value for value in [1, 2]]
        (result, value)
        """,
        "Comprehension variable isolation",
    ),
    ProbeSpec(
        "fstring.conversion_str", "fstring", "value = 3\nf'{value!s}'", "F-string str conversion"
    ),
    ProbeSpec(
        "fstring.conversion_ascii",
        "fstring",
        "value = 'é'\nf'{value!a}'",
        "F-string ascii conversion",
    ),
    ProbeSpec("fstring.alignment", "fstring", "value = 'x'\nf'{value:>4}'", "F-string alignment"),
    ProbeSpec(
        "fstring.float_precision",
        "fstring",
        "value = 3.14159\nf'{value:.2f}'",
        "F-string float precision",
    ),
    ProbeSpec(
        "fstring.dynamic_width",
        "fstring",
        "value = 7\nwidth = 4\nf'{value:0{width}d}'",
        "F-string dynamic format width",
    ),
    ProbeSpec(
        "fstring.escaped_braces",
        "fstring",
        "value = 3\nf'{{value}}={value}'",
        "F-string escaped braces",
    ),
    _probe(
        "class.instance_attribute",
        "class",
        """
        class Item:
            pass
        item = Item()
        item.value = 3
        item.value
        """,
        "Dynamic instance attribute",
    ),
    _probe(
        "class.property",
        "class_protocol",
        """
        class Item:
            def __init__(self, value):
                self._value = value
            @property
            def doubled(self):
                return self._value * 2
        Item(3).doubled
        """,
        "Property descriptor",
    ),
    _probe(
        "class.staticmethod",
        "class_protocol",
        """
        class Math:
            @staticmethod
            def add(left, right):
                return left + right
        Math.add(2, 3)
        """,
        "Static method",
    ),
    _probe(
        "class.classmethod",
        "class_protocol",
        """
        class Item:
            value = 3
            @classmethod
            def get_value(cls):
                return cls.value
        Item.get_value()
        """,
        "Class method",
    ),
    _probe(
        "protocol.callable",
        "class_protocol",
        """
        class Add:
            def __call__(self, left, right):
                return left + right
        Add()(2, 3)
        """,
        "User-defined callable protocol",
    ),
    _probe(
        "protocol.length",
        "class_protocol",
        """
        class Sized:
            def __len__(self):
                return 4
        len(Sized())
        """,
        "User-defined length protocol",
    ),
    _probe(
        "protocol.truthiness",
        "class_protocol",
        """
        class Flag:
            def __bool__(self):
                return False
        bool(Flag())
        """,
        "User-defined truthiness protocol",
    ),
    _probe(
        "protocol.equality",
        "class_protocol",
        """
        class Item:
            def __init__(self, value):
                self.value = value
            def __eq__(self, other):
                return self.value == other.value
        Item(2) == Item(2)
        """,
        "User-defined equality protocol",
    ),
    _probe(
        "protocol.ordering",
        "class_protocol",
        """
        class Item:
            def __init__(self, value):
                self.value = value
            def __lt__(self, other):
                return self.value < other.value
        Item(2) < Item(3)
        """,
        "User-defined ordering protocol",
    ),
    _probe(
        "protocol.getitem",
        "class_protocol",
        """
        class Container:
            def __getitem__(self, key):
                return key * 2
        Container()[3]
        """,
        "User-defined subscription protocol",
    ),
    _probe(
        "protocol.setitem",
        "class_protocol",
        """
        class Container:
            def __init__(self):
                self.value = None
            def __setitem__(self, key, value):
                self.value = (key, value)
        container = Container()
        container[2] = 3
        container.value
        """,
        "User-defined item assignment protocol",
    ),
    _probe(
        "protocol.contains",
        "class_protocol",
        """
        class Container:
            def __contains__(self, value):
                return value == 3
        3 in Container()
        """,
        "User-defined containment protocol",
    ),
    _probe(
        "protocol.string",
        "class_protocol",
        """
        class Item:
            def __str__(self):
                return "item"
        str(Item())
        """,
        "User-defined string conversion protocol",
    ),
    _probe(
        "protocol.repr",
        "class_protocol",
        """
        class Item:
            def __repr__(self):
                return "Item()"
        repr(Item())
        """,
        "User-defined repr protocol",
    ),
    _probe(
        "protocol.hash",
        "class_protocol",
        """
        class Item:
            def __hash__(self):
                return 42
        hash(Item()) == 42
        """,
        "User-defined hash protocol",
    ),
    _probe(
        "protocol.add",
        "class_protocol",
        """
        class Number:
            def __init__(self, value):
                self.value = value
            def __add__(self, other):
                return self.value + other.value
        Number(2) + Number(3)
        """,
        "User-defined addition protocol",
    ),
    _probe(
        "async.gather",
        "async",
        """
        import asyncio
        async def value(number):
            return number
        async def main():
            return await asyncio.gather(value(1), value(2))
        asyncio.run(main())
        """,
        "Async gather",
    ),
    _probe(
        "async.with",
        "async",
        """
        import asyncio
        class Context:
            async def __aenter__(self):
                return 3
            async def __aexit__(self, exc_type, exc, tb):
                return False
        async def main():
            async with Context() as value:
                return value
        asyncio.run(main())
        """,
        "Async context manager protocol",
    ),
    _probe(
        "async.for",
        "async",
        """
        import asyncio
        class AsyncCount:
            def __init__(self):
                self.value = 0
            def __aiter__(self):
                return self
            async def __anext__(self):
                if self.value == 3:
                    raise StopAsyncIteration
                value = self.value
                self.value += 1
                return value
        async def main():
            result = []
            async for value in AsyncCount():
                result.append(value)
            return result
        asyncio.run(main())
        """,
        "Async iterator protocol",
    ),
)
