use std::{
    collections::HashSet,
    env,
    error::Error,
    io::{self, Write},
    process::{Command, Stdio},
};

use monty::MontyRun;
use monty_compat::{CapabilityIndex, DiagnosticDisposition, lower_source};
use monty_types::{CompileOptions, MontyException, MontyObject, NoLimitTracker, PrintWriter};
use serde::Deserialize;

const MANIFEST: &str = include_str!("../../../manifests/monty-v0.0.19.json");

const CPYTHON_RUNNER: &str = r#"
import ast
import contextlib
import io
import json
import sys

source = sys.stdin.read()
stdout = io.StringIO()
stderr = io.StringIO()
try:
    with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
        tree = ast.parse(source, filename="<monty-differential>", mode="exec")
        namespace = {"__name__": "__main__"}
        if tree.body and isinstance(tree.body[-1], ast.Expr):
            prefix = ast.Module(body=tree.body[:-1], type_ignores=tree.type_ignores)
            expression = ast.Expression(tree.body[-1].value)
            exec(compile(prefix, "<monty-differential>", "exec"), namespace)
            value = eval(compile(expression, "<monty-differential>", "eval"), namespace)
        else:
            exec(compile(tree, "<monty-differential>", "exec"), namespace)
            value = None
    outcome = {"kind": "return", "repr": repr(value), "exception_type": None, "message": None}
except BaseException as exc:
    cls = type(exc)
    name = cls.__qualname__
    if cls.__module__ not in ("builtins", "__main__"):
        name = f"{cls.__module__}.{name}"
    outcome = {
        "kind": "raise",
        "repr": None,
        "exception_type": name,
        "message": str(exc),
    }
outcome["stdout"] = stdout.getvalue()
outcome["stderr"] = stderr.getvalue()
json.dump(outcome, sys.stdout, ensure_ascii=False, sort_keys=True)
"#;

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ExecutionEnvelope {
    kind: String,
    repr: Option<String>,
    exception_type: Option<String>,
    message: Option<String>,
    stdout: String,
    stderr: String,
}

impl ExecutionEnvelope {
    fn returned(value: &MontyObject, stdout: String) -> Self {
        Self {
            kind: "return".to_owned(),
            repr: Some(value.py_repr()),
            exception_type: None,
            message: None,
            stdout,
            stderr: String::new(),
        }
    }

    fn raised(exception: &MontyException, stdout: String) -> Self {
        Self {
            kind: "raise".to_owned(),
            repr: None,
            exception_type: Some(exception.exc_type().to_string()),
            message: Some(exception.message().unwrap_or_default().to_owned()),
            stdout,
            stderr: String::new(),
        }
    }
}

struct Fixture {
    name: &'static str,
    expected_rules: &'static [&'static str],
    source: &'static str,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "match_subject_order_sequence_or_guard",
        expected_rules: &["match_statement"],
        source: concat!(
            "events = []\n",
            "def subject():\n",
            "    events.append('subject')\n",
            "    return [1, 2]\n",
            "match subject():\n",
            "    case [first, second] if first < second:\n",
            "        result = ('ordered', first, second)\n",
            "    case 3 | 4:\n",
            "        result = ('small',)\n",
            "    case _:\n",
            "        result = ('other',)\n",
            "(result, events)\n",
        ),
    },
    Fixture {
        name: "match_mapping_pattern",
        expected_rules: &["match_statement"],
        source: concat!(
            "subject = {'kind': 'ok', 'value': 3, 'extra': 4}\n",
            "match subject:\n",
            "    case {'kind': 'ok', 'value': value}:\n",
            "        result = value\n",
            "    case _:\n",
            "        result = 0\n",
            "result\n",
        ),
    },
    Fixture {
        name: "match_class_pattern",
        expected_rules: &["match_statement"],
        source: concat!(
            "class Point:\n",
            "    __match_args__ = ('x', 'y')\n",
            "    def __init__(self, x, y):\n",
            "        self.x = x\n",
            "        self.y = y\n",
            "subject = Point(1, 2)\n",
            "match subject:\n",
            "    case Point(1, y):\n",
            "        result = y\n",
            "    case _:\n",
            "        result = 0\n",
            "result\n",
        ),
    },
    Fixture {
        name: "match_failed_guard_keeps_bindings",
        expected_rules: &["match_statement"],
        source: concat!(
            "events = []\n",
            "match [1]:\n",
            "    case [value] if (events.append(value) or False):\n",
            "        result = 'guard'\n",
            "    case _:\n",
            "        result = ('fallback', value)\n",
            "(result, events)\n",
        ),
    },
    Fixture {
        name: "function_decorator_order",
        expected_rules: &["function_decorator"],
        source: concat!(
            "events = []\n",
            "def decorate(label):\n",
            "    events.append('factory-' + label)\n",
            "    def apply(function):\n",
            "        events.append('apply-' + label)\n",
            "        def wrapped(value):\n",
            "            return function(value) + label\n",
            "        return wrapped\n",
            "    return apply\n",
            "@decorate('A')\n",
            "@decorate('B')\n",
            "def render(value):\n",
            "    return value\n",
            "(render('x'), events)\n",
        ),
    },
    Fixture {
        name: "for_subscript_target",
        expected_rules: &["for_complex_target"],
        source: concat!(
            "state = {'last': 0}\n",
            "for state['last'] in [1, 2, 3]:\n",
            "    pass\n",
            "state\n",
        ),
    },
    Fixture {
        name: "for_attribute_target",
        expected_rules: &["for_complex_target"],
        source: concat!(
            "class State:\n",
            "    pass\n",
            "state = State()\n",
            "for state.last in [1, 2, 3]:\n",
            "    pass\n",
            "state.last\n",
        ),
    },
    Fixture {
        name: "delete_subscript",
        expected_rules: &["delete_subscript"],
        source: "values = [1, 2, 3]\ndel values[1]\nvalues\n",
    },
    Fixture {
        name: "delete_user_subscript",
        expected_rules: &["delete_subscript"],
        source: concat!(
            "class Values:\n",
            "    def __init__(self):\n",
            "        self.items = {'x': 1}\n",
            "    def __delitem__(self, key):\n",
            "        self.items.pop(key)\n",
            "values = Values()\n",
            "del values['x']\n",
            "values.items\n",
        ),
    },
    Fixture {
        name: "assert_message_exception",
        expected_rules: &["assert_message"],
        source: "print('before')\nassert False, 'bad'\n",
    },
    Fixture {
        name: "assert_without_message_exception",
        expected_rules: &["assert_message"],
        source: "assert False\n",
    },
    Fixture {
        name: "dict_union_order_and_inputs",
        expected_rules: &["dict_union"],
        source: concat!(
            "left = {'a': 1, 'same': 1}\n",
            "right = {'same': 2, 'b': 3}\n",
            "merged = left | right\n",
            "(merged, left, right)\n",
        ),
    },
    Fixture {
        name: "bytes_static_iterable",
        expected_rules: &["bytes_iterable"],
        source: "bytes([0, 65, 255])\n",
    },
    Fixture {
        name: "bytes_static_tuple",
        expected_rules: &["bytes_iterable"],
        source: "bytes((0, 10, 92, 255))\n",
    },
    Fixture {
        name: "int_unicode_decimal",
        expected_rules: &["int_unicode_decimal"],
        source: "(int('１２'), int(' ٣ '))\n",
    },
    Fixture {
        name: "callable_iterator_stop",
        expected_rules: &["iter_callable_stop_iteration"],
        source: concat!(
            "values = [1, 2, 0]\n",
            "def take():\n",
            "    return values.pop(0)\n",
            "(list(iter(take, 0)), values)\n",
        ),
    },
    Fixture {
        name: "callable_iterator_raises_stop_iteration",
        expected_rules: &["iter_callable_stop_iteration"],
        source: concat!(
            "values = [1, 2]\n",
            "def take():\n",
            "    if not values:\n",
            "        raise StopIteration\n",
            "    return values.pop(0)\n",
            "(list(iter(take, 0)), values)\n",
        ),
    },
    Fixture {
        name: "late_bound_comprehension_lambdas",
        expected_rules: &["closure_late_binding"],
        source: concat!(
            "functions = [lambda: value for value in range(3)]\n",
            "[function() for function in functions]\n",
        ),
    },
    Fixture {
        name: "dead_map_is_lazy",
        expected_rules: &["dead_lazy_builtin"],
        source: concat!(
            "events = []\n",
            "def visit(value):\n",
            "    events.append(value)\n",
            "    return value\n",
            "unused = map(visit, [1, 2, 3])\n",
            "events\n",
        ),
    },
    Fixture {
        name: "dead_filter_is_lazy",
        expected_rules: &["dead_lazy_builtin"],
        source: concat!(
            "events = []\n",
            "def visit(value):\n",
            "    events.append(value)\n",
            "    return True\n",
            "unused = filter(visit, [1, 2, 3])\n",
            "events\n",
        ),
    },
    Fixture {
        name: "dead_enumerate_is_lazy",
        expected_rules: &["dead_lazy_builtin"],
        source: concat!(
            "events = []\n",
            "values = [1, 2, None]\n",
            "def take():\n",
            "    value = values.pop(0)\n",
            "    events.append(value)\n",
            "    return value\n",
            "unused = enumerate(iter(take, None), 4)\n",
            "events\n",
        ),
    },
    Fixture {
        name: "dead_zip_is_lazy",
        expected_rules: &["dead_lazy_builtin"],
        source: concat!(
            "events = []\n",
            "values = [1, 2, None]\n",
            "def take():\n",
            "    value = values.pop(0)\n",
            "    events.append(value)\n",
            "    return value\n",
            "unused = zip(iter(take, None), [10, 20])\n",
            "events\n",
        ),
    },
    Fixture {
        name: "fstring_user_format",
        expected_rules: &["fstring_user_format"],
        source: concat!(
            "class Label:\n",
            "    def __format__(self, spec):\n",
            "        return spec + '-formatted'\n",
            "label = Label()\n",
            "f'{label:wide}'\n",
        ),
    },
    Fixture {
        name: "class_comprehension_scope",
        expected_rules: &["class_body_comprehension_scope"],
        source: concat!(
            "result = 'not-raised'\n",
            "try:\n",
            "    class Item:\n",
            "        offset = 10\n",
            "        values = [value + offset for value in [1, 2]]\n",
            "except NameError:\n",
            "    result = 'name-error'\n",
            "result\n",
        ),
    },
    Fixture {
        name: "with_exit_snapshotted_once",
        expected_rules: &["with_exit_bound_once"],
        source: concat!(
            "events = []\n",
            "def replacement(self, exc_type, exc, traceback):\n",
            "    events.append('new')\n",
            "class Context:\n",
            "    def __enter__(self):\n",
            "        return self\n",
            "    def __exit__(self, exc_type, exc, traceback):\n",
            "        events.append('old')\n",
            "with Context():\n",
            "    Context.__exit__ = replacement\n",
            "events\n",
        ),
    },
    Fixture {
        name: "with_attribute_target",
        expected_rules: &["with_complex_target", "with_exit_bound_once"],
        source: concat!(
            "class Context:\n",
            "    def __enter__(self):\n",
            "        return 3\n",
            "    def __exit__(self, exc_type, exc, traceback):\n",
            "        return False\n",
            "class State:\n",
            "    pass\n",
            "state = State()\n",
            "with Context() as state.value:\n",
            "    pass\n",
            "state.value\n",
        ),
    },
    Fixture {
        name: "async_with_non_raising_return",
        expected_rules: &["async_with_non_raising_return"],
        source: concat!(
            "import asyncio\n",
            "class Context:\n",
            "    async def __aenter__(self):\n",
            "        return 3\n",
            "    async def __aexit__(self, exc_type, exc, traceback):\n",
            "        return False\n",
            "async def main():\n",
            "    async with Context() as value:\n",
            "        return value\n",
            "asyncio.run(main())\n",
        ),
    },
    Fixture {
        name: "gather_return_exceptions",
        expected_rules: &["async_gather_return_exceptions"],
        source: concat!(
            "import asyncio\n",
            "async def fail():\n",
            "    raise ValueError('bad')\n",
            "async def succeed():\n",
            "    return 3\n",
            "async def main():\n",
            "    return await asyncio.gather(fail(), succeed(), return_exceptions=True)\n",
            "asyncio.run(main())\n",
        ),
    },
    Fixture {
        name: "legacy_and_str_formatting",
        expected_rules: &["percent_format", "str_format"],
        source: "('%s' % 3, '%r' % 'x', '{}'.format(4), '{!r}'.format('y'))\n",
    },
    Fixture {
        name: "ellipsis_builtin_name",
        expected_rules: &["ellipsis_builtin"],
        source: "(Ellipsis is ..., repr(Ellipsis))\n",
    },
    Fixture {
        name: "dead_generator_is_lazy",
        expected_rules: &["dead_generator_expression"],
        source: concat!(
            "events = []\n",
            "def visit(value):\n",
            "    events.append(value)\n",
            "    return value\n",
            "unused = (visit(value) for value in range(3))\n",
            "events\n",
        ),
    },
    Fixture {
        name: "dead_invalid_format_branch",
        expected_rules: &["dead_module_if"],
        source: concat!(
            "if False:\n",
            "    unreachable = f'{1:not-a-real-format}'\n",
            "result = 3\n",
            "result\n",
        ),
    },
    Fixture {
        name: "constant_class_body_if",
        expected_rules: &["class_body_if"],
        source: concat!(
            "class Choice:\n",
            "    if True:\n",
            "        value = 3\n",
            "    else:\n",
            "        value = 4\n",
            "Choice.value\n",
        ),
    },
    Fixture {
        name: "class_tuple_assignment",
        expected_rules: &["class_tuple_assignment"],
        source: concat!(
            "class Pair:\n",
            "    first, second = (1, 2)\n",
            "(Pair.first, Pair.second)\n",
        ),
    },
    Fixture {
        name: "nested_class_binding",
        expected_rules: &["nested_class"],
        source: concat!(
            "class Outer:\n",
            "    class Inner:\n",
            "        value = 3\n",
            "Outer.Inner.value\n",
        ),
    },
    Fixture {
        name: "basic_dataclass",
        expected_rules: &["dataclass_import", "dataclass"],
        source: concat!(
            "from dataclasses import dataclass\n",
            "@dataclass\n",
            "class Point:\n",
            "    x: int\n",
            "    y: int = 2\n",
            "point = Point(3)\n",
            "other = Point(3)\n",
            "(point.x, point.y, point.__eq__(other), repr(point))\n",
        ),
    },
    Fixture {
        name: "inheritance_and_super",
        expected_rules: &["class_inheritance", "super"],
        source: concat!(
            "class Base:\n",
            "    def value(self):\n",
            "        return 'base'\n",
            "    def inherited(self):\n",
            "        return 2\n",
            "class Child(Base):\n",
            "    def value(self):\n",
            "        return super().value() + '-child'\n",
            "child = Child()\n",
            "(child.value(), child.inherited())\n",
        ),
    },
    Fixture {
        name: "property_descriptor",
        expected_rules: &["class_method_decorator", "property"],
        source: concat!(
            "class Box:\n",
            "    @property\n",
            "    def value(self):\n",
            "        return 4\n",
            "box = Box()\n",
            "box.value\n",
        ),
    },
    Fixture {
        name: "classmethod_descriptor",
        expected_rules: &["class_method_decorator", "classmethod"],
        source: concat!(
            "class Box:\n",
            "    @classmethod\n",
            "    def label(cls, suffix):\n",
            "        return 'box' + suffix\n",
            "Box.label('!')\n",
        ),
    },
    Fixture {
        name: "classmethod_through_instance",
        expected_rules: &["class_method_decorator", "classmethod"],
        source: concat!(
            "class Box:\n",
            "    @classmethod\n",
            "    def label(cls, suffix):\n",
            "        return cls.__name__ + suffix\n",
            "box = Box()\n",
            "box.label('!')\n",
        ),
    },
    Fixture {
        name: "classmethod_complex_receiver_evaluated_once",
        expected_rules: &["class_method_decorator", "classmethod"],
        source: concat!(
            "events = []\n",
            "class Box:\n",
            "    def __init__(self):\n",
            "        events.append('init')\n",
            "    @classmethod\n",
            "    def label(cls, suffix):\n",
            "        return 'box' + suffix\n",
            "result = Box().label('!')\n",
            "(result, events)\n",
        ),
    },
    Fixture {
        name: "staticmethod_descriptor",
        expected_rules: &["class_method_decorator", "staticmethod"],
        source: concat!(
            "class Box:\n",
            "    @staticmethod\n",
            "    def plus(value):\n",
            "        return value + 1\n",
            "box = Box()\n",
            "box.plus(2)\n",
        ),
    },
    Fixture {
        name: "staticmethod_complex_receiver_evaluated_once",
        expected_rules: &["class_method_decorator", "staticmethod"],
        source: concat!(
            "events = []\n",
            "class Box:\n",
            "    def __init__(self):\n",
            "        events.append('init')\n",
            "    @staticmethod\n",
            "    def plus(value):\n",
            "        return value + 1\n",
            "result = Box().plus(2)\n",
            "(result, events)\n",
        ),
    },
    Fixture {
        name: "user_class_protocols",
        expected_rules: &[
            "protocol_binary",
            "protocol_truthiness",
            "protocol_length",
            "protocol_hash",
            "protocol_callable",
            "protocol_getitem",
            "protocol_contains",
            "protocol_compare",
        ],
        source: concat!(
            "class Box:\n",
            "    def __add__(self, other):\n",
            "        return 10 + other\n",
            "    def __bool__(self):\n",
            "        return False\n",
            "    def __len__(self):\n",
            "        return 3\n",
            "    def __hash__(self):\n",
            "        return 7\n",
            "    def __call__(self, value):\n",
            "        return value * 2\n",
            "    def __getitem__(self, index):\n",
            "        return index * 3\n",
            "    def __contains__(self, value):\n",
            "        return value == 2\n",
            "    def __eq__(self, other):\n",
            "        return True\n",
            "    def __lt__(self, other):\n",
            "        return True\n",
            "box = Box()\n",
            "(box + 2, bool(box), len(box), hash(box), box(4), box[2], 2 in box, box == Box(), box < Box())\n",
        ),
    },
    Fixture {
        name: "user_class_binary_protocol_matrix",
        expected_rules: &["protocol_binary"],
        source: concat!(
            "class Box:\n",
            "    def __sub__(self, other):\n        return 1\n",
            "    def __mul__(self, other):\n        return 2\n",
            "    def __matmul__(self, other):\n        return 3\n",
            "    def __truediv__(self, other):\n        return 4\n",
            "    def __floordiv__(self, other):\n        return 5\n",
            "    def __mod__(self, other):\n        return 6\n",
            "    def __pow__(self, other):\n        return 7\n",
            "    def __lshift__(self, other):\n        return 8\n",
            "    def __rshift__(self, other):\n        return 9\n",
            "    def __and__(self, other):\n        return 10\n",
            "    def __xor__(self, other):\n        return 11\n",
            "    def __or__(self, other):\n        return 12\n",
            "(Box() - 1, Box() * 1, Box() @ 1, Box() / 1, Box() // 1, Box() % 1, Box() ** 1, Box() << 1, Box() >> 1, Box() & 1, Box() ^ 1, Box() | 1)\n",
        ),
    },
    Fixture {
        name: "user_class_unary_protocol_matrix",
        expected_rules: &["protocol_unary"],
        source: concat!(
            "class Box:\n",
            "    def __neg__(self):\n        return 1\n",
            "    def __pos__(self):\n        return 2\n",
            "    def __invert__(self):\n        return 3\n",
            "(-Box(), +Box(), ~Box())\n",
        ),
    },
    Fixture {
        name: "user_class_round_and_reversed_protocols",
        expected_rules: &["protocol_round", "protocol_reversed"],
        source: concat!(
            "class Box:\n",
            "    def __round__(self):\n        return 17\n",
            "    def __reversed__(self):\n        return iter([3, 2, 1])\n",
            "box = Box()\n",
            "(round(box), list(reversed(box)))\n",
        ),
    },
    Fixture {
        name: "user_class_iterator",
        expected_rules: &["protocol_iterator"],
        source: concat!(
            "class Counter:\n",
            "    def __init__(self):\n",
            "        self.index = 0\n",
            "    def __iter__(self):\n",
            "        return self\n",
            "    def __next__(self):\n",
            "        if self.index >= 3:\n",
            "            raise StopIteration\n",
            "        value = self.index\n",
            "        self.index += 1\n",
            "        return value\n",
            "list(Counter())\n",
        ),
    },
    Fixture {
        name: "truthiness_falls_back_to_length",
        expected_rules: &["protocol_truthiness"],
        source: concat!(
            "class Empty:\n",
            "    def __len__(self):\n",
            "        return 0\n",
            "class Full:\n",
            "    def __len__(self):\n",
            "        return 2\n",
            "(bool(Empty()), bool(Full()))\n",
        ),
    },
    Fixture {
        name: "reflected_comparison_and_negative_membership",
        expected_rules: &["protocol_compare", "protocol_contains"],
        source: concat!(
            "class Box:\n",
            "    def __gt__(self, other):\n",
            "        return other == 3\n",
            "    def __contains__(self, value):\n",
            "        return value == 4\n",
            "box = Box()\n",
            "(3 < box, 3 not in box, 4 not in box)\n",
        ),
    },
    Fixture {
        name: "user_class_setitem_getitem",
        expected_rules: &["protocol_setitem", "protocol_getitem"],
        source: concat!(
            "class Box:\n",
            "    def __init__(self):\n",
            "        self.values = {}\n",
            "    def __setitem__(self, key, value):\n",
            "        self.values[key] = value\n",
            "    def __getitem__(self, key):\n",
            "        return self.values[key]\n",
            "box = Box()\n",
            "box['answer'] = 42\n",
            "box['answer']\n",
        ),
    },
    Fixture {
        name: "user_class_setattr",
        expected_rules: &["class_setattr"],
        source: concat!(
            "events = []\n",
            "class Box:\n",
            "    def __setattr__(self, name, value):\n",
            "        events.append((name, value))\n",
            "box = Box()\n",
            "box.answer = 42\n",
            "events\n",
        ),
    },
    Fixture {
        name: "user_class_getattr",
        expected_rules: &["class_getattr"],
        source: concat!(
            "class Box:\n",
            "    def __getattr__(self, name):\n",
            "        return 'missing-' + name\n",
            "box = Box()\n",
            "box.answer\n",
        ),
    },
    Fixture {
        name: "private_name_mangling",
        expected_rules: &["private_name_mangling"],
        source: concat!(
            "class Secret:\n",
            "    __value = 3\n",
            "    def read(self):\n",
            "        return self.__value\n",
            "Secret().read()\n",
        ),
    },
    Fixture {
        name: "mutable_class_name",
        expected_rules: &["class_assign_name", "class_name_access"],
        source: concat!(
            "class Box:\n",
            "    pass\n",
            "before = Box.__name__\n",
            "Box.__name__ = 'Renamed'\n",
            "(before, Box.__name__)\n",
        ),
    },
    Fixture {
        name: "class_metatype_identity",
        expected_rules: &["class_type_identity", "class_isinstance_type"],
        source: concat!(
            "class Box:\n",
            "    pass\n",
            "(type(Box) is type, isinstance(Box, type))\n",
        ),
    },
    Fixture {
        name: "bound_method_identity_and_type",
        expected_rules: &["bound_method_type", "bound_method_equality"],
        source: concat!(
            "class Box:\n",
            "    def method(self):\n",
            "        return 1\n",
            "first = Box()\n",
            "second = Box()\n",
            "(repr(type(first.method)), first.method == first.method, first.method == second.method)\n",
        ),
    },
    Fixture {
        name: "default_repr_qualification",
        expected_rules: &["class_default_repr"],
        source: concat!(
            "class Box:\n",
            "    pass\n",
            "repr(Box()).startswith('<__main__.Box object')\n",
        ),
    },
    Fixture {
        name: "shared_nan_sequence_ordering",
        expected_rules: &["nan_shared_sequence"],
        source: concat!(
            "value = float('nan')\n",
            "([value] < [value], [value] <= [value], [value] > [value], [value] >= [value])\n",
        ),
    },
];

fn literal_arguments_after(source: &str, marker: &str) -> HashSet<String> {
    source
        .split(marker)
        .skip(1)
        .filter_map(|tail| {
            let tail = tail.trim_start();
            let literal = tail.strip_prefix('"')?;
            let end = literal.find('"')?;
            Some(literal[..end].to_owned())
        })
        .collect()
}

fn implemented_applied_rules() -> HashSet<String> {
    let source = include_str!("../src/lower.rs");
    let mut rules = literal_arguments_after(source, "self.applied(");
    rules.extend(literal_arguments_after(source, "self.replace_expression("));

    for tail in source
        .split("self.diagnostics.push(LoweringDiagnostic {")
        .skip(1)
    {
        let Some(end) = tail.find("});") else {
            continue;
        };
        let diagnostic = &tail[..end];
        if !diagnostic.contains("disposition: DiagnosticDisposition::Applied") {
            continue;
        }
        rules.extend(literal_arguments_after(diagnostic, "rule:"));
    }
    rules
}

#[test]
fn lowered_fixtures_match_cpython_and_monty() -> Result<(), Box<dyn Error>> {
    let python = find_cpython()?;
    let capabilities = CapabilityIndex::from_json(MANIFEST)?;
    let mut covered_rules = HashSet::new();

    for fixture in FIXTURES {
        let lowered = lower_source(fixture.source, &capabilities)?;
        assert!(lowered.changed, "{} did not trigger lowering", fixture.name);
        assert!(
            lowered
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.disposition == DiagnosticDisposition::Applied),
            "{} emitted a non-applied diagnostic: {:#?}",
            fixture.name,
            lowered.diagnostics
        );
        for expected_rule in fixture.expected_rules {
            assert!(
                lowered.diagnostics.iter().any(|diagnostic| {
                    diagnostic.rule == *expected_rule
                        && diagnostic.disposition == DiagnosticDisposition::Applied
                }),
                "{} did not apply expected rule {}: {:#?}",
                fixture.name,
                expected_rule,
                lowered.diagnostics
            );
            covered_rules.insert(*expected_rule);
        }

        let original_cpython = run_cpython(&python, fixture.source)?;
        let lowered_cpython = run_cpython(&python, &lowered.code)?;
        assert_eq!(
            original_cpython, lowered_cpython,
            "{} changed CPython semantics\n--- lowered ---\n{}",
            fixture.name, lowered.code
        );

        let lowered_monty = run_monty(&lowered.code);
        assert_eq!(
            lowered_cpython, lowered_monty,
            "{} differs between CPython and Monty\n--- lowered ---\n{}",
            fixture.name, lowered.code
        );
    }

    let declared_rules: HashSet<_> = FIXTURES
        .iter()
        .flat_map(|fixture| fixture.expected_rules.iter().copied())
        .collect();
    assert_eq!(covered_rules, declared_rules);

    let covered_rules: HashSet<_> = covered_rules.into_iter().map(str::to_owned).collect();
    assert_eq!(
        covered_rules,
        implemented_applied_rules(),
        "every literal Applied lowering rule must have a differential fixture"
    );
    Ok(())
}

fn find_cpython() -> Result<String, Box<dyn Error>> {
    let mut candidates = Vec::new();
    if let Ok(configured) = env::var("MONTY_COMPAT_CPYTHON") {
        candidates.push(configured);
    }
    candidates.extend(
        [
            "python3.14",
            "python3.13",
            "python3.12",
            "python3.11",
            "python3",
        ]
        .into_iter()
        .map(str::to_owned),
    );

    for candidate in candidates {
        let available = Command::new(&candidate)
            .args([
                "-c",
                "import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if available {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "differential tests require CPython 3.11+; set MONTY_COMPAT_CPYTHON",
    )
    .into())
}

fn run_cpython(python: &str, source: &str) -> Result<ExecutionEnvelope, Box<dyn Error>> {
    let mut child = Command::new(python)
        .args(["-I", "-c", CPYTHON_RUNNER])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err(io::Error::other("CPython child has no stdin pipe").into());
    };
    stdin.write_all(source.as_bytes())?;
    drop(stdin);

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "CPython oracle failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    serde_json::from_slice(&output.stdout).map_err(Into::into)
}

fn run_monty(source: &str) -> ExecutionEnvelope {
    let mut stdout = String::new();
    let runner = match MontyRun::new(
        source.to_owned(),
        "<monty-differential>",
        Vec::new(),
        CompileOptions::default(),
    ) {
        Ok(runner) => runner,
        Err(exception) => return ExecutionEnvelope::raised(&exception, stdout),
    };
    match runner.run(
        Vec::new(),
        NoLimitTracker,
        PrintWriter::collect_string(&mut stdout),
    ) {
        Ok(value) => ExecutionEnvelope::returned(&value, stdout),
        Err(exception) => ExecutionEnvelope::raised(&exception, stdout),
    }
}
