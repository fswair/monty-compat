"""Core logic: parse Monty Rust source → capability sets + code checker."""

from __future__ import annotations

import ast
import io
import re
import zipfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any
from urllib.request import urlopen

# ── GitHub source locations ──────────────────────────────────────────
_GITHUB_ZIP = "https://github.com/pydantic/monty/archive/refs/heads/main.zip"
_BUILTINS_REL = "crates/monty/src/builtins/mod.rs"
_MODULES_REL = "crates/monty/src/modules/mod.rs"
_MODULES_DIR_REL = "crates/monty/src/modules"
_TYPES_REL = "crates/monty/src/types/type.rs"
_EXCEPTIONS_REL = "crates/monty/src/exception_private.rs"
_INTERN_REL = "crates/monty/src/intern.rs"


# ══════════════════════════════════════════════════════════════════════
# Rust source parsers
# ══════════════════════════════════════════════════════════════════════


def _pascal_to_snake(name: str) -> str:
    """Convert PascalCase → snake_case (matching strum's serialize_all)."""
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", name)
    s = re.sub(r"([a-z\d])([A-Z])", r"\1_\2", s)
    return s.lower()


def _parse_static_strings_map(intern_src: str) -> dict[str, str]:
    """Parse the ``StaticStrings`` enum in intern.rs → ``{variant: python_string}``.

    Respects ``#[strum(serialize_all = "snake_case")]`` as the default and
    explicit ``#[strum(serialize = "...")]`` annotations as overrides.
    """
    m = re.search(r"pub enum StaticStrings \{(.*?)\n\}", intern_src, re.DOTALL)
    if not m:
        return {}

    body = m.group(1)
    result: dict[str, str] = {}
    pending_serialize: str | None = None

    for line in body.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        # Explicit serialize override
        sm = re.match(r'#\[strum\(serialize\s*=\s*"([^"]+)"\)\]', stripped)
        if sm:
            pending_serialize = sm.group(1)
            continue
        # Skip other attributes and comments
        if stripped.startswith("#") or stripped.startswith("//"):
            pending_serialize = None
            continue
        # Variant declaration
        vm = re.match(r"^([A-Z]\w*)", stripped)
        if vm:
            variant = vm.group(1)
            result[variant] = (
                pending_serialize if pending_serialize is not None else _pascal_to_snake(variant)
            )
            pending_serialize = None

    return result


def _parse_strum_enum_variants(source: str, enum_name: str) -> list[str]:
    """Extract uncommented variant names from any Rust enum.

    Handles both ``pub enum`` and ``pub(crate) enum``.
    """
    pattern = rf"pub(?:\([^)]*\))?\s+enum\s+{enum_name}\s*\{{(.*?)\}}"
    m = re.search(pattern, source, re.DOTALL)
    if not m:
        return []

    body = m.group(1)
    variants: list[str] = []
    for line in body.splitlines():
        line = line.strip()
        if not line or line.startswith("//") or line.startswith("#"):
            continue
        vm = re.match(r"^([A-Z][A-Za-z0-9_]*)", line)
        if vm:
            variants.append(vm.group(1))
    return variants


def _parse_builtin_functions(source: str) -> set[str]:
    """Parse ``BuiltinsFunctions`` enum → lowercase Python function names."""
    # BuiltinsFunctions has its own serialize_all = "lowercase"
    return {v.lower() for v in _parse_strum_enum_variants(source, "BuiltinsFunctions")}


def _parse_builtin_modules(source: str) -> set[str]:
    """Parse ``BuiltinModule`` enum → module name strings."""
    names: set[str] = set()
    for m in re.finditer(r"StaticStrings::(\w+)\s*=>\s*Some\(Self::", source):
        names.add(m.group(1).lower())
    if not names:
        names = {v.lower() for v in _parse_strum_enum_variants(source, "BuiltinModule")}
    return names


def _parse_type_constructors(source: str) -> set[str]:
    """Parse ``Type::from_builtin_name`` match arms → type constructor names."""
    names: set[str] = set()
    for m in re.finditer(r'"(\w+)"\s*=>\s*Some\(Self::', source):
        names.add(m.group(1))
    return names


def _parse_exception_types(source: str) -> set[str]:
    """Parse ``ExcType`` enum variants → Python exception class names."""
    return set(_parse_strum_enum_variants(source, "ExcType"))


def _parse_module_functions_enum(source: str, enum_name: str) -> set[str]:
    """Parse a module-specific function enum → lowercase function names."""
    return {v.lower() for v in _parse_strum_enum_variants(source, enum_name)}


def _parse_module_attributes(source: str, static_strings: dict[str, str]) -> set[str]:
    """Extract attribute names registered on a Monty module.

    Handles three patterns found in the Monty codebase:

    1. Direct call: ``module.set_attr(StaticStrings::Xxx, ...)``
    2. Loop over array: ``for ss in SOME_ARRAY { module.set_attr(*ss, ...) }``
       → scans ``const SOME_ARRAY: &[StaticStrings] = &[ ... ]``
    3. Inline array in the for body (handled the same way)
    """
    names: set[str] = set()

    # Pattern 1: direct set_attr calls
    for m in re.finditer(r"module\.set_attr\(\s*\*?StaticStrings::(\w+)", source):
        variant = m.group(1)
        if variant in static_strings:
            names.add(static_strings[variant])

    # Pattern 2+3: `for ss in ARRAY_IDENT { module.set_attr(*ss, ...) }` loops
    # Find the array identifier used in the loop header
    matches = re.finditer(r"for\s+\w+\s+in\s+(\w+)\s*\{[^}]*module\.set_attr", source, re.DOTALL)
    for loop_m in matches:
        array_name = loop_m.group(1)
        # Now find the const/static slice with that name
        array_pattern = rf"(?:const|static)\s+{re.escape(array_name)}\s*:[^=]*=\s*&\[(.*?)\]"
        am = re.search(array_pattern, source, re.DOTALL)
        if am:
            for vm in re.finditer(r"StaticStrings::(\w+)", am.group(1)):
                variant = vm.group(1)
                if variant in static_strings:
                    names.add(static_strings[variant])

    return names


# ══════════════════════════════════════════════════════════════════════
# Source bundle (all files needed for parsing)
# ══════════════════════════════════════════════════════════════════════


@dataclass
class _Sources:
    builtins: str
    modules: str
    types: str
    exceptions: str
    intern: str
    module_files: dict[str, str]

    @classmethod
    def from_local(cls, root: Path) -> _Sources:
        mod_files: dict[str, str] = {}
        modules_dir = root / _MODULES_DIR_REL
        if modules_dir.is_dir():
            for p in sorted(modules_dir.glob("*.rs")):
                if p.stem != "mod":
                    mod_files[p.stem] = p.read_text()
        return cls(
            builtins=(root / _BUILTINS_REL).read_text(),
            modules=(root / _MODULES_REL).read_text(),
            types=(root / _TYPES_REL).read_text(),
            exceptions=(root / _EXCEPTIONS_REL).read_text(),
            intern=(root / _INTERN_REL).read_text(),
            module_files=mod_files,
        )

    @classmethod
    def from_zip(cls, zf: zipfile.ZipFile, prefix: str) -> _Sources:
        def read(rel: str) -> str:
            return zf.read(prefix + rel).decode()

        mod_files: dict[str, str] = {}
        modules_prefix = prefix + _MODULES_DIR_REL + "/"
        for zi in zf.infolist():
            if zi.filename.startswith(modules_prefix) and zi.filename.endswith(".rs"):
                stem = zi.filename[len(modules_prefix):].rstrip("/")
                if stem and "/" not in stem and stem != "mod.rs":
                    mod_files[stem[:-3]] = zf.read(zi.filename).decode()
        return cls(
            builtins=read(_BUILTINS_REL),
            modules=read(_MODULES_REL),
            types=read(_TYPES_REL),
            exceptions=read(_EXCEPTIONS_REL),
            intern=read(_INTERN_REL),
            module_files=mod_files,
        )


def _build_from_sources(src: _Sources) -> MontyCapabilities:
    """Turn parsed Rust source files into a :class:`MontyCapabilities`."""
    ss_map = _parse_static_strings_map(src.intern)

    # Per-module: merge function-enum names + set_attr-scanned attribute names
    mod_attrs: dict[str, frozenset[str]] = {}
    for mod_name, mod_src in src.module_files.items():
        enum_name = mod_name.capitalize() + "Functions"
        funcs = _parse_module_functions_enum(mod_src, enum_name)
        attrs = _parse_module_attributes(mod_src, ss_map)
        combined = funcs | attrs
        if combined:
            mod_attrs[mod_name] = frozenset(combined)

    return MontyCapabilities(
        builtin_functions=frozenset(_parse_builtin_functions(src.builtins)),
        type_constructors=frozenset(_parse_type_constructors(src.types)),
        exception_types=frozenset(_parse_exception_types(src.exceptions)),
        modules=frozenset(_parse_builtin_modules(src.modules)),
        module_attributes=mod_attrs,
    )


# ══════════════════════════════════════════════════════════════════════
# Capability container
# ══════════════════════════════════════════════════════════════════════


@dataclass(frozen=True)
class MontyCapabilities:
    """What the Monty sandbox currently supports.

    Built by parsing the Rust source — either from a local checkout or
    downloaded from GitHub.  Instances are immutable and hashable so they
    can be safely cached.
    """

    builtin_functions: frozenset[str] = field(default_factory=frozenset)
    """Built-in functions available without an import (abs, len, print, …)."""

    type_constructors: frozenset[str] = field(default_factory=frozenset)
    """Type names that act as constructors (int, str, list, dict, …)."""

    exception_types: frozenset[str] = field(default_factory=frozenset)
    """Exception classes available as builtins (ValueError, TypeError, …)."""

    modules: frozenset[str] = field(default_factory=frozenset)
    """Importable stdlib modules: ``{'sys', 'typing', 'asyncio', 'pathlib', 'os'}``."""

    module_attributes: dict[str, frozenset[str]] = field(default_factory=dict)
    """Attributes/functions available inside each supported module.

    ``{'asyncio': {'gather', 'run'}, 'os': {'getenv'}, 'sys': {'version', …}, …}``
    """

    # ── Constructors ─────────────────────────────────────────────────

    @classmethod
    def from_local(cls, monty_root: str | Path) -> MontyCapabilities:
        """Build from a local Monty repo checkout."""
        return _build_from_sources(_Sources.from_local(Path(monty_root)))

    @classmethod
    def from_github(cls, url: str = _GITHUB_ZIP, *, branch: str = "main") -> MontyCapabilities:
        """Download the Monty repo as a ZIP archive and parse capabilities in memory."""
        with urlopen(url) as resp:  # noqa: S310
            data = resp.read()
        zf = zipfile.ZipFile(io.BytesIO(data))
        return _build_from_sources(_Sources.from_zip(zf, f"monty-{branch}/"))

    # ── Cache-backed class-level accessors ────────────────────────────

    @classmethod
    def _cached(cls, *, cache: bool = True) -> MontyCapabilities:
        """Load capabilities from cache (or rebuild if *cache* is False)."""
        from .cache import get_capabilities
        return get_capabilities(cache="auto" if cache else "regenerate")

    @classmethod
    def get_modules(cls, *, cache: bool = True) -> frozenset[str]:
        """Return the set of importable stdlib module names Monty supports.

        Args:
            cache: Set to ``False`` to discard the on-disk cache and rebuild
                from the GitHub source before returning.

        Example::

            MontyCapabilities.get_modules()
            # frozenset({'asyncio', 'os', 'pathlib', 're', 'sys', 'typing'})
        """
        return cls._cached(cache=cache).modules

    @classmethod
    def get_builtins(cls, *, cache: bool = True) -> frozenset[str]:
        """Return the set of builtin function names Monty has implemented.

        Args:
            cache: Set to ``False`` to discard the on-disk cache and rebuild.

        Example::

            MontyCapabilities.get_builtins()
            # frozenset({'abs', 'all', 'any', 'bin', 'chr', …})
        """
        return cls._cached(cache=cache).builtin_functions

    @classmethod
    def get_types(cls, *, cache: bool = True) -> frozenset[str]:
        """Return the set of type constructor names available as builtins.

        Args:
            cache: Set to ``False`` to discard the on-disk cache and rebuild.

        Example::

            MontyCapabilities.get_types()
            # frozenset({'bool', 'bytes', 'dict', 'float', 'frozenset', …})
        """
        return cls._cached(cache=cache).type_constructors

    @classmethod
    def get_exception_types(cls, *, cache: bool = True) -> frozenset[str]:
        """Return the set of exception class names Monty supports.

        Args:
            cache: Set to ``False`` to discard the on-disk cache and rebuild.

        Example::

            MontyCapabilities.get_exception_types()
            # frozenset({'ValueError', 'TypeError', 'RuntimeError', …})
        """
        return cls._cached(cache=cache).exception_types

    @classmethod
    def get_attrs_of_module(
        cls,
        module: str,
        *,
        cache: bool = True,
    ) -> frozenset[str]:
        """Return the set of attributes/functions available inside *module*.

        Returns an empty frozenset if the module is not supported or has no
        known attribute data.

        Args:
            module: Module name, e.g. ``'asyncio'``, ``'os'``, ``'typing'``.
            cache: Set to ``False`` to discard the on-disk cache and rebuild.

        Example::

            MontyCapabilities.get_attrs_of_module('asyncio')
            # frozenset({'gather', 'run'})

            MontyCapabilities.get_attrs_of_module('typing')
            # frozenset({'Any', 'Optional', 'Union', 'List', …})
        """
        caps = cls._cached(cache=cache)
        return caps.module_attributes.get(module, frozenset())

    # ── JSON serialisation ────────────────────────────────────────────

    def to_dict(self) -> dict[str, Any]:
        """Serialise to a plain dict (JSON-safe)."""
        return {
            "builtin_functions": sorted(self.builtin_functions),
            "type_constructors": sorted(self.type_constructors),
            "exception_types": sorted(self.exception_types),
            "modules": sorted(self.modules),
            "module_attributes": {k: sorted(v) for k, v in sorted(self.module_attributes.items())},
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MontyCapabilities:
        """Restore from a plain dict (e.g. loaded from JSON)."""
        return cls(
            builtin_functions=frozenset(data.get("builtin_functions", [])),
            type_constructors=frozenset(data.get("type_constructors", [])),
            exception_types=frozenset(data.get("exception_types", [])),
            modules=frozenset(data.get("modules", [])),
            module_attributes={
                k: frozenset(v) for k, v in data.get("module_attributes", {}).items()
            },
        )

    # ── Code analysis ─────────────────────────────────────────────────

    def check_code(self, code: str) -> tuple[bool, list[str]]:
        """Check whether *code* can run in the Monty sandbox.

        Parses with Python's ``ast`` module and reports:
        - ``import X`` where X is not a supported module
        - ``from X import Y`` where X is not supported, or Y is not in
          that module's known attribute set
        - Calls to CPython builtins that Monty has not implemented

        Returns ``(can_run, reasons)``.  An empty *reasons* list means
        Monty should handle it fine.
        """
        try:
            tree = ast.parse(code)
        except SyntaxError:
            return True, []

        reasons: list[str] = []
        self._check_node(tree, reasons)
        return len(reasons) == 0, reasons

    def _check_node(self, node: ast.AST, reasons: list[str]) -> None:
        if isinstance(node, ast.ClassDef):
            reasons.append(f"class definitions are not supported by Monty ('{node.name}')")
            return  # no need to descend into the class body

        elif isinstance(node, ast.Import):
            for alias in node.names:
                top = alias.name.split(".")[0]
                if top not in self.modules:
                    reasons.append(f"module '{alias.name}' is not supported by Monty")

        elif isinstance(node, ast.ImportFrom):
            if node.module:
                top = node.module.split(".")[0]
                if top not in self.modules:
                    reasons.append(f"module '{node.module}' is not supported by Monty")
                else:
                    # Validate individual imported names when we have attribute data
                    known = self.module_attributes.get(top)
                    if known:
                        for alias in node.names:
                            if alias.name != "*" and alias.name not in known:
                                reasons.append(
                                    f"'{alias.name}' is not available in Monty's '{top}' module"
                                )

        elif isinstance(node, ast.Call):
            if isinstance(node.func, ast.Name):
                name = node.func.id
                if name in _PYTHON_BUILTINS and name not in self._all_names:
                    reasons.append(f"builtin '{name}' is not implemented in Monty")

        for child in ast.iter_child_nodes(node):
            self._check_node(child, reasons)

    @property
    def _all_names(self) -> frozenset[str]:
        return self.builtin_functions | self.type_constructors | self.exception_types

    # ── Pretty printing ───────────────────────────────────────────────

    def summary(self) -> str:
        """Return a human-readable summary of capabilities."""
        lines = ["Monty Sandbox Capabilities", "=" * 40]

        lines.append(f"\nBuiltin Functions ({len(self.builtin_functions)}):")
        for name in sorted(self.builtin_functions):
            lines.append(f"  - {name}")

        lines.append(f"\nType Constructors ({len(self.type_constructors)}):")
        for name in sorted(self.type_constructors):
            lines.append(f"  - {name}")

        lines.append(f"\nException Types ({len(self.exception_types)}):")
        for name in sorted(self.exception_types):
            lines.append(f"  - {name}")

        lines.append(f"\nModules ({len(self.modules)}):")
        for name in sorted(self.modules):
            attrs = self.module_attributes.get(name, frozenset())
            if attrs:
                lines.append(f"  - {name}:")
                for attr in sorted(attrs):
                    lines.append(f"      · {attr}")
            else:
                lines.append(f"  - {name}")

        return "\n".join(lines)


# ── CPython callable builtins we check against ────────────────────────
_PYTHON_BUILTINS: frozenset[str] = frozenset(
    {
        "abs",
        "aiter",
        "all",
        "anext",
        "any",
        "ascii",
        "bin",
        "bool",
        "breakpoint",
        "bytearray",
        "bytes",
        "callable",
        "chr",
        "classmethod",
        "compile",
        "complex",
        "delattr",
        "dict",
        "dir",
        "divmod",
        "enumerate",
        "eval",
        "exec",
        "filter",
        "float",
        "format",
        "frozenset",
        "getattr",
        "globals",
        "hasattr",
        "hash",
        "help",
        "hex",
        "id",
        "input",
        "int",
        "isinstance",
        "issubclass",
        "iter",
        "len",
        "list",
        "locals",
        "map",
        "max",
        "memoryview",
        "min",
        "next",
        "object",
        "oct",
        "open",
        "ord",
        "pow",
        "print",
        "property",
        "range",
        "repr",
        "reversed",
        "round",
        "set",
        "setattr",
        "slice",
        "sorted",
        "staticmethod",
        "str",
        "sum",
        "super",
        "tuple",
        "type",
        "vars",
        "zip",
        "__import__",
    }
)
