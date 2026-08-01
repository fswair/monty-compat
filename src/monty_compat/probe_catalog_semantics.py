"""Targeted probes for Python semantics that matter to source lowering."""

from __future__ import annotations

from textwrap import dedent

from .probes import ProbeSpec


def _probe(id: str, category: str, source: str, description: str) -> ProbeSpec:
    return ProbeSpec(id, category, dedent(source).strip(), description)


SEMANTIC_PROBES: tuple[ProbeSpec, ...] = (
    _probe(
        "builtin.all_short_circuit",
        "builtin",
        """
        calls = []
        values = [True, False, True, None]
        def take():
            value = values.pop(0)
            calls.append(value)
            return value
        result = all(iter(take, None))
        (result, calls)
        """,
        "all() iterator consumption and short circuiting",
    ),
    _probe(
        "builtin.any_short_circuit",
        "builtin",
        """
        calls = []
        values = [False, True, False, None]
        def take():
            value = values.pop(0)
            calls.append(value)
            return value
        result = any(iter(take, None))
        (result, calls)
        """,
        "any() iterator consumption and short circuiting",
    ),
    ProbeSpec(
        "builtin.enumerate_start",
        "builtin",
        "list(enumerate(['a', 'b'], start=4))",
        "enumerate() with a keyword start",
    ),
    ProbeSpec(
        "builtin.filter_none",
        "builtin",
        "list(filter(None, [0, 1, '', 2]))",
        "filter() using truthiness without a callback",
    ),
    _probe(
        "builtin.filter_callback",
        "builtin",
        """
        def even(value):
            return value % 2 == 0
        list(filter(even, range(6)))
        """,
        "filter() with a Python callback",
    ),
    _probe(
        "builtin.map_multiple_iterables",
        "builtin",
        """
        def add(left, right):
            return left + right
        list(map(add, [1, 2], [10, 20, 30]))
        """,
        "map() over multiple iterables",
    ),
    _probe(
        "builtin.map_lazy",
        "builtin_semantics",
        """
        calls = []
        def visit(value):
            calls.append(value)
            return value
        values = map(visit, [1, 2, 3])
        calls
        """,
        "Lazy evaluation of map() callbacks",
    ),
    _probe(
        "builtin.filter_lazy",
        "builtin_semantics",
        """
        calls = []
        def visit(value):
            calls.append(value)
            return True
        values = filter(visit, [1, 2, 3])
        calls
        """,
        "Lazy evaluation of filter() callbacks",
    ),
    _probe(
        "builtin.enumerate_lazy",
        "builtin_semantics",
        """
        calls = []
        values = [1, 2, None]
        def take():
            value = values.pop(0)
            calls.append(value)
            return value
        indexed = enumerate(iter(take, None))
        calls
        """,
        "Lazy consumption by enumerate()",
    ),
    _probe(
        "builtin.zip_lazy",
        "builtin_semantics",
        """
        calls = []
        values = [1, 2, None]
        def take():
            value = values.pop(0)
            calls.append(value)
            return value
        pairs = zip(iter(take, None), [10, 20])
        calls
        """,
        "Lazy consumption by zip()",
    ),
    _probe(
        "builtin.sorted_key_reverse",
        "builtin",
        """
        values = ["aaa", "b", "cc"]
        sorted(values, key=lambda value: len(value), reverse=True)
        """,
        "sorted() with key and reverse keyword arguments",
    ),
    _probe(
        "builtin.min_key",
        "builtin",
        """
        values = ["aaa", "b", "cc"]
        min(values, key=lambda value: len(value))
        """,
        "min() with a key callback",
    ),
    ProbeSpec(
        "builtin.max_default",
        "builtin",
        "max([], default=7)",
        "max() default for an empty iterable",
    ),
    ProbeSpec(
        "builtin.zip_strict_equal",
        "builtin",
        "list(zip([1, 2], ['a', 'b'], strict=True))",
        "zip(strict=True) with equal-length iterables",
    ),
    _probe(
        "builtin.zip_strict_mismatch",
        "builtin",
        """
        try:
            list(zip([1], [2, 3], strict=True))
        except ValueError:
            result = "value-error"
        except TypeError:
            result = "type-error"
        else:
            result = "no-error"
        result
        """,
        "zip(strict=True) length mismatch behavior",
    ),
    ProbeSpec(
        "builtin.reversed_tuple",
        "builtin",
        "list(reversed((1, 2, 3)))",
        "reversed() over a tuple",
    ),
    _probe(
        "builtin.next_default",
        "builtin",
        """
        iterator = iter([])
        next(iterator, "empty")
        """,
        "next() default after exhaustion",
    ),
    _probe(
        "builtin.iter_identity",
        "builtin",
        """
        iterator = iter([1, 2])
        iter(iterator) is iterator
        """,
        "iter(iterator) preserves iterator identity",
    ),
    _probe(
        "builtin.iter_callable_sentinel",
        "builtin",
        """
        values = [1, 2, 3]
        def take():
            return values.pop(0)
        list(iter(take, 3))
        """,
        "Two-argument iter() with a callable and sentinel",
    ),
    _probe(
        "builtin.iter_callable_stop_iteration",
        "builtin",
        """
        def take():
            raise StopIteration
        try:
            list(iter(take, 3))
        except StopIteration:
            result = "escaped"
        else:
            result = "stopped"
        result
        """,
        "StopIteration raised by iter(callable, sentinel)",
    ),
    ProbeSpec(
        "builtin.range_negative_step",
        "builtin",
        "list(range(5, -1, -2))",
        "range() with a negative step",
    ),
    ProbeSpec(
        "builtin.range_slice",
        "builtin",
        "list(range(10)[1:8:2])",
        "Slicing a range object",
    ),
    _probe(
        "builtin.bytes_iterable",
        "builtin",
        """
        try:
            result = bytes([65, 66])
        except TypeError:
            result = "type-error"
        result
        """,
        "bytes() construction from an iterable of integers",
    ),
    _probe(
        "builtin.int_unicode_decimal",
        "builtin",
        """
        try:
            result = int("١٢")
        except ValueError:
            result = "value-error"
        result
        """,
        "int() parsing of non-ASCII decimal digits",
    ),
    ProbeSpec(
        "builtin.round_half_even",
        "builtin",
        "(round(2.5), round(3.5), round(1.25, 1))",
        "Banker's rounding behavior",
    ),
    ProbeSpec(
        "builtin.pow_modulo",
        "builtin",
        "pow(3, 4, 5)",
        "Three-argument modular pow()",
    ),
    ProbeSpec(
        "builtin.divmod_negative",
        "builtin",
        "divmod(-7, 3)",
        "divmod() floor semantics for negative values",
    ),
    ProbeSpec(
        "builtin.sum_start",
        "builtin",
        "sum([1, 2, 3], 10)",
        "sum() with a non-zero start",
    ),
    _probe(
        "builtin.getattr_default",
        "builtin",
        """
        class Item:
            pass
        getattr(Item(), "missing", 7)
        """,
        "getattr() default for a missing attribute",
    ),
    _probe(
        "builtin.hasattr_missing",
        "builtin",
        """
        class Item:
            pass
        hasattr(Item(), "missing")
        """,
        "hasattr() for a missing instance attribute",
    ),
    _probe(
        "class.dynamic_type",
        "class",
        """
        Dynamic = type("Dynamic", (), {"value": 3})
        (Dynamic.__name__, Dynamic().value)
        """,
        "Dynamic class construction with three-argument type()",
    ),
    _probe(
        "class.dynamic_type_method",
        "class",
        """
        def value(self):
            return self.number
        Dynamic = type("Dynamic", (), {"number": 3, "value": value})
        Dynamic().value()
        """,
        "Method binding on a dynamically-created class",
    ),
    _probe(
        "class.variable_expression",
        "class",
        """
        class Item:
            base = 3
            doubled = base * 2
        Item.doubled
        """,
        "Class variable expression referencing an earlier class variable",
    ),
    _probe(
        "class.init_parameter_shapes",
        "class",
        """
        class Item:
            def __init__(self, first, /, second=2, *rest, flag, **extra):
                self.values = (first, second, rest, flag, extra["answer"])
        Item(1, 3, 4, 5, flag=True, answer=42).values
        """,
        "Full parameter shapes on __init__",
    ),
    _probe(
        "class.function_attribute_binding",
        "class",
        """
        def value(self):
            return self.number
        class Item:
            pass
        Item.value = value
        item = Item()
        item.number = 4
        item.value()
        """,
        "Function assigned to a class becomes a bound method",
    ),
    _probe(
        "class.setattr_class",
        "class",
        """
        class Item:
            pass
        setattr(Item, "answer", 42)
        Item.answer
        """,
        "setattr() on a class object",
    ),
    _probe(
        "class.setattr_instance",
        "class",
        """
        class Item:
            pass
        item = Item()
        setattr(item, "answer", 42)
        item.answer
        """,
        "setattr() on a class instance",
    ),
    _probe(
        "class.type_identity",
        "class_semantics",
        """
        class Item:
            pass
        type(Item) is type
        """,
        "Whether a user class object is an instance of type",
    ),
    _probe(
        "class.isinstance_type",
        "class_semantics",
        """
        class Item:
            pass
        try:
            result = isinstance(Item, type)
        except TypeError:
            result = "type-error"
        result
        """,
        "isinstance(user_class, type) behavior",
    ),
    _probe(
        "class.bound_method_call",
        "class",
        """
        class Item:
            def value(self):
                return 3
        method = Item().value
        method()
        """,
        "Calling a stored bound method",
    ),
    _probe(
        "class.bound_method_type",
        "class_semantics",
        """
        class Item:
            def value(self):
                return 3
        repr(type(Item().value))
        """,
        "Runtime type reported for a bound method",
    ),
    _probe(
        "class.bound_method_equality",
        "class_semantics",
        """
        class Item:
            def value(self):
                return 3
        item = Item()
        item.value == item.value
        """,
        "Equality of two bound-method accesses",
    ),
    _probe(
        "class.docstrings",
        "class",
        '''
        class Item:
            """item docs"""
        (Item.__doc__, Item().__doc__)
        ''',
        "Class and instance docstring access",
    ),
    _probe(
        "class.default_repr_qualified",
        "class_semantics",
        """
        class Item:
            pass
        repr(Item()).startswith("<__main__.Item object")
        """,
        "Module qualification in the default instance repr",
    ),
    _probe(
        "class.decorator_order",
        "class",
        """
        events = []
        def decorate(name):
            events.append("eval-" + name)
            def apply(cls):
                events.append("apply-" + name)
                return cls
            return apply
        @decorate("outer")
        @decorate("inner")
        class Item:
            pass
        events
        """,
        "Class decorator evaluation and application order",
    ),
    _probe(
        "class.ellipsis_body",
        "class",
        """
        class Item:
            ...
        Item.__name__
        """,
        "Ellipsis as an empty class body",
    ),
    _probe(
        "class.annotated_variable",
        "class",
        """
        class Item:
            answer: int = 42
        Item.answer
        """,
        "Annotated class variable assignment",
    ),
    _probe(
        "class.enclosing_closure",
        "class",
        """
        def make(value):
            class Item:
                answer = value
            return Item().answer
        make(42)
        """,
        "Class body capture from an enclosing function",
    ),
    _probe(
        "class.private_name_mangling",
        "class_semantics",
        """
        class Item:
            __answer = 42
            def answer(self):
                return self.__answer
        (Item().answer(), hasattr(Item, "_Item__answer"))
        """,
        "Double-underscore private-name mangling in a class",
    ),
    _probe(
        "class.object_class_identity",
        "class",
        """
        class Item:
            pass
        item = Item()
        (item.__class__ is Item, type(item) is Item, isinstance(item, Item))
        """,
        "Class identity through __class__, type(), and isinstance()",
    ),
    _probe(
        "class.assign_name",
        "class_semantics",
        """
        class Item:
            pass
        Item.__name__ = "Renamed"
        Item.__name__
        """,
        "Assigning a new user-class __name__",
    ),
    _probe(
        "class.assign_object_class",
        "class_semantics",
        """
        class First:
            pass
        class Second:
            pass
        item = First()
        item.__class__ = Second
        (item.__class__ is Second, type(item) is Second)
        """,
        "Assigning an instance __class__",
    ),
    _probe(
        "class.body_comprehension_scope",
        "class_semantics",
        """
        try:
            class Item:
                offset = 10
                values = [value + offset for value in [1, 2]]
        except NameError:
            result = "name-error"
        else:
            result = Item.values
        result
        """,
        "Visibility of class variables inside a class-body comprehension",
    ),
    _probe(
        "class.body_if",
        "class",
        """
        class Item:
            if True:
                value = 3
        Item.value
        """,
        "If statement inside a class body",
    ),
    _probe(
        "class.body_tuple_assignment",
        "class",
        """
        class Item:
            left, right = (2, 3)
        Item.left + Item.right
        """,
        "Destructuring assignment inside a class body",
    ),
    _probe(
        "class.nested_class",
        "class",
        """
        class Outer:
            class Inner:
                value = 3
        Outer.Inner.value
        """,
        "Nested class definition inside a class body",
    ),
    _probe(
        "class.getattr_hook",
        "class_semantics",
        """
        class Item:
            def __getattr__(self, name):
                return "hook-" + name
        try:
            result = Item().missing
        except AttributeError:
            result = "attribute-error"
        result
        """,
        "User-defined __getattr__ dispatch",
    ),
    _probe(
        "class.setattr_hook",
        "class_semantics",
        """
        events = []
        class Item:
            def __setattr__(self, name, value):
                events.append((name, value))
        item = Item()
        item.answer = 42
        events
        """,
        "User-defined __setattr__ dispatch",
    ),
    _probe(
        "comprehension.generator_lazy",
        "comprehension_semantics",
        """
        events = []
        def visit(value):
            events.append(value)
            return value
        values = (visit(value) for value in range(3))
        events
        """,
        "Generator-expression lazy evaluation",
    ),
    _probe(
        "comprehension.generator_type",
        "comprehension_semantics",
        """
        values = (value for value in range(3))
        type(values) is list
        """,
        "Generator-expression runtime type",
    ),
    _probe(
        "comprehension.evaluation_order",
        "comprehension",
        """
        events = []
        def visit(value):
            events.append(value)
            return value
        result = [visit(value) for value in [1, 2, 3] if visit(value * 10)]
        (result, events)
        """,
        "Comprehension filter and element evaluation order",
    ),
    _probe(
        "comprehension.leftmost_iterable_once",
        "comprehension",
        """
        calls = []
        def values():
            calls.append("values")
            return [1, 2]
        result = [value for value in values()]
        (result, calls)
        """,
        "Single evaluation of a comprehension's leftmost iterable",
    ),
    _probe(
        "function.default_evaluated_once",
        "function_semantics",
        """
        calls = []
        def make_default():
            calls.append("default")
            return []
        def append(value, items=make_default()):
            items.append(value)
            return items
        (append(1), append(2), calls)
        """,
        "Function defaults are evaluated once at definition time",
    ),
    _probe(
        "function.call_argument_order",
        "function_semantics",
        """
        events = []
        def visit(value):
            events.append(value)
            return value
        def combine(first, second, third):
            return first + second + third
        value = combine(visit(1), third=visit(3), second=visit(2))
        (value, events)
        """,
        "Left-to-right evaluation of positional and keyword arguments",
    ),
    _probe(
        "function.closure_late_binding",
        "function_semantics",
        """
        functions = [lambda: value for value in range(3)]
        [function() for function in functions]
        """,
        "Late binding of comprehension variables in closures",
    ),
    _probe(
        "function.loop_closure_late_binding",
        "function_semantics",
        """
        functions = []
        for value in range(3):
            functions.append(lambda: value)
        [function() for function in functions]
        """,
        "Late binding of for-loop variables in closures",
    ),
    _probe(
        "function.closure_default_capture",
        "function_semantics",
        """
        functions = [lambda value=value: value for value in range(3)]
        [function() for function in functions]
        """,
        "Capturing comprehension variables through lambda defaults",
    ),
    _probe(
        "statement.for_attribute_target",
        "statement",
        """
        class Item:
            pass
        item = Item()
        for item.value in [1, 2, 3]:
            pass
        item.value
        """,
        "Attribute assignment target in a for loop",
    ),
    _probe(
        "statement.for_subscript_target",
        "statement",
        """
        values = {}
        for values["last"] in [1, 2, 3]:
            pass
        values["last"]
        """,
        "Subscript assignment target in a for loop",
    ),
    _probe(
        "operator.dict_union",
        "operator",
        """
        left = {"a": 1, "shared": 1}
        right = {"b": 2, "shared": 3}
        left | right
        """,
        "Dictionary union operator",
    ),
    _probe(
        "operator.set_algebra",
        "operator",
        """
        left = {1, 2, 3}
        right = {3, 4}
        (sorted(left | right), sorted(left & right), sorted(left - right), sorted(left ^ right))
        """,
        "Set union, intersection, difference, and symmetric difference",
    ),
    ProbeSpec(
        "operator.sequence_concat_repeat",
        "operator",
        "([1, 2] + [3]) * 2",
        "List concatenation and repetition",
    ),
    ProbeSpec(
        "operator.sequence_lexicographic",
        "operator",
        "((1, 2) < (1, 3), [2] > [1, 9])",
        "Lexicographic ordering of tuples and lists",
    ),
    _probe(
        "operator.nan_shared_sequence",
        "operator_semantics",
        """
        value = float("nan")
        [1, value] < [1, value, 3]
        """,
        "Sequence ordering when a shared prefix contains NaN",
    ),
    _probe(
        "exception.tuple_handler",
        "exception",
        """
        try:
            int("x")
        except (TypeError, ValueError):
            result = "caught"
        result
        """,
        "Tuple of exception types in an except handler",
    ),
    _probe(
        "exception.subclass_handler",
        "exception",
        """
        try:
            raise FileNotFoundError("missing")
        except OSError:
            result = "caught"
        result
        """,
        "Catching an exception through a built-in parent class",
    ),
    _probe(
        "exception.bare_reraise",
        "exception",
        """
        try:
            try:
                raise ValueError("inner")
            except ValueError:
                raise
        except ValueError as error:
            result = str(error)
        result
        """,
        "Bare raise rethrows the active exception",
    ),
    _probe(
        "exception.finally_return_override",
        "exception",
        """
        def value():
            try:
                return "try"
            finally:
                return "finally"
        value()
        """,
        "Return from finally overrides a pending return",
    ),
    _probe(
        "exception.finally_loop_control",
        "exception",
        """
        events = []
        for value in range(3):
            try:
                if value == 1:
                    continue
                events.append(value)
            finally:
                events.append("finally-" + str(value))
        events
        """,
        "Finally execution across continue in a loop",
    ),
    _probe(
        "exception.arguments",
        "exception",
        """
        empty = ValueError()
        message = ValueError("bad")
        (empty.args, str(empty), message.args, str(message), repr(message))
        """,
        "Exception args, str, and repr behavior",
    ),
    _probe(
        "exception.assert_message",
        "exception",
        """
        value = 2
        try:
            assert value == 3
        except AssertionError as error:
            result = str(error)
        result
        """,
        "Generated message for a failed comparison assertion",
    ),
    _probe(
        "exception.explicit_cause",
        "exception_semantics",
        """
        try:
            try:
                raise ValueError("inner")
            except ValueError as inner:
                raise TypeError("outer") from inner
        except TypeError as outer:
            try:
                cause = outer.__cause__
            except AttributeError:
                result = "missing"
            else:
                result = str(cause)
        result
        """,
        "Preservation of an explicit exception cause",
    ),
    _probe(
        "with.exception_suppression",
        "with",
        """
        events = []
        class Context:
            def __enter__(self):
                events.append("enter")
            def __exit__(self, exc_type, exc, tb):
                events.append("exit")
                return True
        with Context():
            raise ValueError("hidden")
        events
        """,
        "Truthy __exit__ suppresses an exception",
    ),
    _probe(
        "with.exception_arguments",
        "with_semantics",
        """
        observed = []
        class Context:
            def __enter__(self):
                return self
            def __exit__(self, exc_type, exc, tb):
                observed.extend([exc_type is ValueError, str(exc), tb is None])
                return True
        with Context():
            raise ValueError("bad")
        observed
        """,
        "Exception type, value, and traceback passed to __exit__",
    ),
    _probe(
        "with.exit_bound_once",
        "with_semantics",
        """
        events = []
        def replacement(self, exc_type, exc, tb):
            events.append("new")
        class Context:
            def __enter__(self):
                return self
            def __exit__(self, exc_type, exc, tb):
                events.append("old")
        with Context():
            Context.__exit__ = replacement
        events
        """,
        "Whether __exit__ is bound once before executing the with body",
    ),
    _probe(
        "with.return_runs_exit",
        "with",
        """
        events = []
        class Context:
            def __enter__(self):
                events.append("enter")
            def __exit__(self, exc_type, exc, tb):
                events.append("exit")
        def run():
            with Context():
                return 3
        value = run()
        (value, events)
        """,
        "A return inside with still executes __exit__",
    ),
    _probe(
        "with.loop_control_runs_exit",
        "with",
        """
        events = []
        class Context:
            def __init__(self, value):
                self.value = value
            def __enter__(self):
                events.append("enter-" + str(self.value))
            def __exit__(self, exc_type, exc, tb):
                events.append("exit-" + str(self.value))
        for value in range(3):
            with Context(value):
                if value == 0:
                    continue
                break
        events
        """,
        "Break and continue inside with still execute __exit__",
    ),
    _probe(
        "with.attribute_target",
        "with",
        """
        class Item:
            pass
        class Context:
            def __enter__(self):
                return 3
            def __exit__(self, exc_type, exc, tb):
                pass
        item = Item()
        with Context() as item.value:
            pass
        item.value
        """,
        "Attribute assignment target in a with-as clause",
    ),
    _probe(
        "with.subscript_target",
        "with",
        """
        class Context:
            def __enter__(self):
                return 3
            def __exit__(self, exc_type, exc, tb):
                pass
        values = {}
        with Context() as values["answer"]:
            pass
        values["answer"]
        """,
        "Subscript assignment target in a with-as clause",
    ),
    _probe(
        "with.empty_list_target",
        "with",
        """
        class Context:
            def __enter__(self):
                return []
            def __exit__(self, exc_type, exc, tb):
                pass
        with Context() as []:
            result = "ok"
        result
        """,
        "Empty-list destructuring target in a with-as clause",
    ),
    _probe(
        "with.empty_tuple_target",
        "with",
        """
        class Context:
            def __enter__(self):
                return ()
            def __exit__(self, exc_type, exc, tb):
                pass
        with Context() as ():
            result = "ok"
        result
        """,
        "Empty-tuple destructuring target in a with-as clause",
    ),
    ProbeSpec(
        "fstring.alternate_hex",
        "fstring",
        "value = 255\nf'{value:#06x}'",
        "Alternate hexadecimal form with zero padding",
    ),
    ProbeSpec(
        "fstring.thousands_separator",
        "fstring",
        "value = 1234567\nf'{value:,d}'",
        "Thousands separator formatting",
    ),
    ProbeSpec(
        "fstring.percentage",
        "fstring",
        "value = 0.125\nf'{value:.1%}'",
        "Percentage formatting",
    ),
    ProbeSpec(
        "fstring.dynamic_precision",
        "fstring",
        "value = 3.14159\nprecision = 3\nf'{value:.{precision}f}'",
        "Dynamically nested precision format specifier",
    ),
    _probe(
        "fstring.custom_format",
        "fstring_semantics",
        """
        class Item:
            def __format__(self, spec):
                return "custom-" + spec
        f"{Item()}"
        """,
        "User-defined __format__ dispatch",
    ),
    _probe(
        "fstring.user_class_spec",
        "fstring_semantics",
        """
        class Item:
            def __str__(self):
                return "item"
        try:
            result = f"{Item():>8}"
        except TypeError:
            result = "type-error"
        result
        """,
        "Format specifier applied to a user-class instance",
    ),
    _probe(
        "fstring.invalid_static_spec_dead_code",
        "fstring_semantics",
        """
        if False:
            value = f"{1:kk}"
        "ok"
        """,
        "Validation timing for a malformed static format specifier",
    ),
    _probe(
        "format.percent_string",
        "format",
        """
        try:
            result = "%s" % 3
        except TypeError:
            result = "type-error"
        result
        """,
        "Printf-style percent string formatting",
    ),
    _probe(
        "format.str_format",
        "format",
        """
        try:
            result = "{}".format(3)
        except AttributeError:
            result = "attribute-error"
        result
        """,
        "str.format() formatting",
    ),
    _probe(
        "async.nested_await",
        "async",
        """
        import asyncio
        async def child(value):
            return value + 1
        async def parent():
            return await child(await child(1))
        asyncio.run(parent())
        """,
        "Nested coroutine calls and await expressions",
    ),
    _probe(
        "async.await_nonawaitable",
        "async",
        """
        import asyncio
        async def main():
            try:
                await 3
            except TypeError:
                return "type-error"
            return "no-error"
        asyncio.run(main())
        """,
        "Awaiting a non-awaitable value",
    ),
    _probe(
        "async.coroutine_single_shot",
        "async",
        """
        import asyncio
        async def child():
            return 3
        async def main():
            coroutine = child()
            await coroutine
            try:
                await coroutine
            except RuntimeError:
                return "runtime-error"
            return "no-error"
        asyncio.run(main())
        """,
        "A coroutine object cannot be awaited twice",
    ),
    _probe(
        "async.gather_return_exceptions",
        "async_semantics",
        """
        import asyncio
        async def fail():
            raise ValueError("bad")
        async def main():
            try:
                values = await asyncio.gather(fail(), return_exceptions=True)
            except NotImplementedError:
                return "unsupported"
            return isinstance(values[0], ValueError)
        asyncio.run(main())
        """,
        "asyncio.gather(return_exceptions=True)",
    ),
)
