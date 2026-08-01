"""Deterministic semantic matrix for user-defined Python protocols."""

from __future__ import annotations

from .probes import ProbeSpec

_BINARY_PROTOCOLS: tuple[tuple[str, str, str], ...] = (
    ("sub", "__sub__", "Box() - 1"),
    ("mul", "__mul__", "Box() * 1"),
    ("matmul", "__matmul__", "Box() @ 1"),
    ("truediv", "__truediv__", "Box() / 1"),
    ("floordiv", "__floordiv__", "Box() // 1"),
    ("mod", "__mod__", "Box() % 1"),
    ("pow", "__pow__", "Box() ** 1"),
    ("lshift", "__lshift__", "Box() << 1"),
    ("rshift", "__rshift__", "Box() >> 1"),
    ("bitand", "__and__", "Box() & 1"),
    ("bitxor", "__xor__", "Box() ^ 1"),
    ("bitor", "__or__", "Box() | 1"),
)

_REFLECTED_PROTOCOLS: tuple[tuple[str, str, str], ...] = (
    ("add", "__radd__", "1 + Box()"),
    ("sub", "__rsub__", "1 - Box()"),
    ("mul", "__rmul__", "1 * Box()"),
    ("matmul", "__rmatmul__", "1 @ Box()"),
    ("truediv", "__rtruediv__", "1 / Box()"),
    ("floordiv", "__rfloordiv__", "1 // Box()"),
    ("mod", "__rmod__", "1 % Box()"),
    ("pow", "__rpow__", "1 ** Box()"),
    ("lshift", "__rlshift__", "1 << Box()"),
    ("rshift", "__rrshift__", "1 >> Box()"),
    ("bitand", "__rand__", "1 & Box()"),
    ("bitxor", "__rxor__", "1 ^ Box()"),
    ("bitor", "__ror__", "1 | Box()"),
)

_UNARY_PROTOCOLS: tuple[tuple[str, str, str], ...] = (
    ("neg", "__neg__", "-Box()"),
    ("pos", "__pos__", "+Box()"),
    ("invert", "__invert__", "~Box()"),
)


def _binary_probe(
    family: str, index: int, slug: str, method: str, expression: str
) -> ProbeSpec:
    expected = 1_000 + index
    return ProbeSpec(
        f"protocol.{family}.{slug}",
        "protocol_matrix",
        f"class Box:\n    def {method}(self, other):\n"
        f"        return {expected}\n{expression}",
        f"User-class {method} operator dispatch",
    )


def _unary_probe(index: int, slug: str, method: str, expression: str) -> ProbeSpec:
    expected = 2_000 + index
    return ProbeSpec(
        f"protocol.unary.{slug}",
        "protocol_matrix",
        f"class Box:\n    def {method}(self):\n"
        f"        return {expected}\n{expression}",
        f"User-class {method} unary dispatch",
    )


PROTOCOL_MATRIX_PROBES: tuple[ProbeSpec, ...] = (
    tuple(
        _binary_probe("binary", index, slug, method, expression)
        for index, (slug, method, expression) in enumerate(_BINARY_PROTOCOLS)
    )
    + tuple(
        _binary_probe("reflected", index, slug, method, expression)
        for index, (slug, method, expression) in enumerate(_REFLECTED_PROTOCOLS)
    )
    + tuple(
        _unary_probe(index, slug, method, expression)
        for index, (slug, method, expression) in enumerate(_UNARY_PROTOCOLS)
    )
    + (
        ProbeSpec(
            "protocol.conversion.int",
            "protocol_matrix",
            "class Box:\n    def __int__(self):\n        return 17\nint(Box())",
            "User-class __int__ conversion",
        ),
        ProbeSpec(
            "protocol.conversion.float",
            "protocol_matrix",
            "class Box:\n    def __float__(self):\n        return 1.5\nfloat(Box())",
            "User-class __float__ conversion",
        ),
        ProbeSpec(
            "protocol.conversion.index",
            "protocol_matrix",
            "class Box:\n    def __index__(self):\n        return 17\nhex(Box())",
            "User-class __index__ conversion",
        ),
        ProbeSpec(
            "protocol.round",
            "protocol_matrix",
            "class Box:\n    def __round__(self):\n        return 17\nround(Box())",
            "User-class __round__ dispatch",
        ),
        ProbeSpec(
            "protocol.reversed",
            "protocol_matrix",
            "class Box:\n    def __reversed__(self):\n"
            "        return iter([3, 2, 1])\nlist(reversed(Box()))",
            "User-class __reversed__ dispatch",
        ),
    )
)
