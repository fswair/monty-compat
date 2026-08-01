"""Parse Monty's Rust source into a Python compatibility capability graph."""

from __future__ import annotations

import json
import re
import zipfile
from collections.abc import Iterator
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

# ── GitHub source locations ──────────────────────────────────────────
_GITHUB_ZIP = "https://github.com/pydantic/monty/archive/refs/heads/main.zip"
_SOURCE_DIR_REL = "crates/monty/src"
_MODULES_REL = f"{_SOURCE_DIR_REL}/modules/mod.rs"
_TYPES_REL = f"{_SOURCE_DIR_REL}/types/type.rs"
_INTERN_REL = f"{_SOURCE_DIR_REL}/intern.rs"
_BUILTINS_RELS = (
    "crates/monty-types/src/builtins.rs",
    f"{_SOURCE_DIR_REL}/builtins/mod.rs",
)
_EXCEPTIONS_RELS = (
    "crates/monty-types/src/exceptions.rs",
    f"{_SOURCE_DIR_REL}/exception_private.rs",
)


# ══════════════════════════════════════════════════════════════════════
# Small Rust source scanner
# ══════════════════════════════════════════════════════════════════════


def _pascal_to_snake(name: str) -> str:
    """Convert PascalCase to snake_case (matching strum's serialize_all)."""
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", name)
    s = re.sub(r"([a-z\d])([A-Z])", r"\1_\2", s)
    return s.lower()


def _matching_delimiter(source: str, start: int, opening: str, closing: str) -> int | None:
    """Return the offset of a balanced Rust delimiter, ignoring strings/comments."""
    if start >= len(source) or source[start] != opening:
        return None

    depth = 0
    index = start
    while index < len(source):
        char = source[index]
        next_two = source[index : index + 2]

        if next_two == "//":
            newline = source.find("\n", index + 2)
            index = len(source) if newline == -1 else newline + 1
            continue
        if next_two == "/*":
            end = source.find("*/", index + 2)
            index = len(source) if end == -1 else end + 2
            continue
        if char == '"':
            quote = '"'
            index += 1
            while index < len(source):
                if source[index] == "\\":
                    index += 2
                    continue
                if source[index] == quote:
                    index += 1
                    break
                index += 1
            continue

        if char == opening:
            depth += 1
        elif char == closing:
            depth -= 1
            if depth == 0:
                return index
        index += 1
    return None


def _delimited_contents(source: str, start: int, opening: str, closing: str) -> str | None:
    """Return the text inside a balanced delimiter beginning at *start*."""
    end = _matching_delimiter(source, start, opening, closing)
    if end is None:
        return None
    return source[start + 1 : end]


def _iter_rust_function_bodies(source: str) -> Iterator[tuple[str, str]]:
    """Yield Rust function names and balanced bodies from a source file."""
    for match in re.finditer(r"\bfn\s+([A-Za-z_]\w*)\b", source):
        params_start = source.find("(", match.end())
        if params_start == -1:
            continue
        params_end = _matching_delimiter(source, params_start, "(", ")")
        if params_end is None:
            continue
        body_start = source.find("{", params_end + 1)
        if body_start == -1:
            continue
        body = _delimited_contents(source, body_start, "{", "}")
        if body is not None:
            yield match.group(1), body


def _function_bodies(source: str, name: str) -> list[str]:
    """Return every body for a Rust function named *name*."""
    return [
        body for function_name, body in _iter_rust_function_bodies(source) if function_name == name
    ]


def _parse_static_strings_map(intern_src: str) -> dict[str, str]:
    """Parse ``StaticStrings`` into ``{RustVariant: python_name}``."""
    match = re.search(r"\bpub(?:\([^)]*\))?\s+enum\s+StaticStrings\s*\{", intern_src)
    if not match:
        return {}
    body = _delimited_contents(intern_src, intern_src.find("{", match.start()), "{", "}")
    if body is None:
        return {}

    names: dict[str, str] = {}
    pending_serialize: str | None = None
    for line in body.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("//"):
            continue
        serialize = re.match(r'#\[strum\(serialize\s*=\s*"([^"]*)"\)\]', stripped)
        if serialize:
            pending_serialize = serialize.group(1)
            continue
        if stripped.startswith("#"):
            continue
        variant = re.match(r"([A-Z]\w*)", stripped)
        if variant:
            name = variant.group(1)
            names[name] = (
                pending_serialize if pending_serialize is not None else _pascal_to_snake(name)
            )
            pending_serialize = None
    return names


def _parse_strum_enum_variants(source: str, enum_name: str) -> list[str]:
    """Extract uncommented Rust enum variants without relying on line formatting."""
    match = re.search(rf"\bpub(?:\([^)]*\))?\s+enum\s+{re.escape(enum_name)}\s*\{{", source)
    if not match:
        return []
    body = _delimited_contents(source, source.find("{", match.start()), "{", "}")
    if body is None:
        return []

    variants: list[str] = []
    for line in body.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith(("//", "#")):
            continue
        variant = re.match(r"([A-Z][A-Za-z0-9_]*)", stripped)
        if variant:
            variants.append(variant.group(1))
    return variants


def _parse_builtin_functions(source: str) -> set[str]:
    """Parse the ``BuiltinsFunctions`` enum into Python function names."""
    return {variant.lower() for variant in _parse_strum_enum_variants(source, "BuiltinsFunctions")}


def _parse_builtin_type_variants(source: str) -> dict[str, str]:
    """Map ``Type`` variants to source-level builtin constructor names."""
    names: dict[str, str] = {}
    for body in _function_bodies(source, "from_builtin_name"):
        for match in re.finditer(r'"([^"]+)"\s*=>\s*Some\(Self::(\w+)\)', body):
            names[match.group(2)] = match.group(1)
    return names


def _without_feature_gated_items(source: str) -> str:
    """Mask items disabled in Monty's default, featureless build.

    Capability extraction describes the production sandbox, not Monty's
    ``test-hooks`` build. Rust's ``#[cfg(feature = ...)]`` can decorate a
    module, a match arm, or an individual ``module.set_attr`` call, so simply
    ignoring the annotated line is insufficient. This scanner masks the
    attribute and the next syntactic item while retaining newlines (useful for
    source positions in diagnostics).
    """

    masked = list(source)
    for match in re.finditer(r"#\[cfg\s*\(\s*feature\s*=\s*[^)]+\)\]", source):
        start = match.start()
        item_start = match.end()
        while item_start < len(source) and source[item_start].isspace():
            item_start += 1

        # A cfg can be followed by additional attributes (for example
        # ``#[doc(hidden)]``) before the Rust item it gates.
        while source.startswith("#[", item_start):
            attribute_end = _matching_delimiter(source, item_start + 1, "[", "]")
            if attribute_end is None:
                break
            item_start = attribute_end + 1
            while item_start < len(source) and source[item_start].isspace():
                item_start += 1

        item_end = item_start
        index = item_start
        paren_depth = bracket_depth = brace_depth = 0
        while index < len(source):
            char = source[index]
            if source[index : index + 2] == "//":
                newline = source.find("\n", index + 2)
                index = len(source) if newline == -1 else newline + 1
                continue
            if source[index : index + 2] == "/*":
                comment_end = source.find("*/", index + 2)
                index = len(source) if comment_end == -1 else comment_end + 2
                continue
            if char == '"':
                index += 1
                while index < len(source):
                    if source[index] == "\\":
                        index += 2
                    elif source[index] == '"':
                        index += 1
                        break
                    else:
                        index += 1
                continue
            if char == "(":
                paren_depth += 1
            elif char == ")":
                paren_depth -= 1
            elif char == "[":
                bracket_depth += 1
            elif char == "]":
                bracket_depth -= 1
            elif char == "{":
                brace_depth += 1
            elif char == "}":
                if brace_depth == 1:
                    item_end = index + 1
                    break
                brace_depth -= 1
            elif char in ";," and not (paren_depth or bracket_depth or brace_depth):
                item_end = index + 1
                break
            index += 1
        else:
            item_end = len(source)

        for position in range(start, item_end):
            if masked[position] != "\n":
                masked[position] = " "
    return "".join(masked)


def _parse_builtin_modules(source: str, static_strings: dict[str, str]) -> set[str]:
    """Parse Monty's standard-library enum and its string-to-module mapping."""
    names = {
        static_strings[match.group(1)]
        for match in re.finditer(r"StaticStrings::(\w+)\s*=>\s*Some\(Self::", source)
        if match.group(1) in static_strings
    }
    if names:
        return names
    for enum_name in ("StandardLib", "BuiltinModule"):
        variants = _parse_strum_enum_variants(source, enum_name)
        if variants:
            return {variant.lower() for variant in variants}
    return set()


def _parse_exception_types(source: str) -> set[str]:
    """Parse ``ExcType`` enum variants into Python exception class names."""
    return set(_parse_strum_enum_variants(source, "ExcType"))


def _iter_module_set_attr_calls(source: str) -> Iterator[tuple[str, str]]:
    """Yield the contents and first ``StaticStrings`` variant of module.set_attr calls."""
    for match in re.finditer(r"\bmodule\.set_attr\s*\(", source):
        open_paren = source.find("(", match.start())
        args = _delimited_contents(source, open_paren, "(", ")")
        if args is None:
            continue
        first_argument = re.match(r"\s*\*?StaticStrings::(\w+)", args)
        if first_argument:
            yield first_argument.group(1), args


def _static_string_arrays(source: str) -> dict[str, set[str]]:
    """Find named Rust arrays/slices containing ``StaticStrings`` values."""
    arrays: dict[str, set[str]] = {}
    pattern = re.compile(r"\b(?:const|static|let)\s+(\w+)\b")
    for match in pattern.finditer(source):
        assignment = source.find("=", match.end())
        if assignment == -1:
            continue
        start = source.find("[", assignment)
        if start == -1:
            continue
        values = _delimited_contents(source, start, "[", "]")
        if values is not None:
            # Registration tables store the public attribute name as the first
            # tuple item. Later StaticStrings entries are commonly *values*
            # (for example ``os.pardir`` is ``".."``) and must not become
            # exported attribute names.
            tuple_keys = re.findall(r"\(\s*StaticStrings::(\w+)", values)
            arrays[match.group(1)] = set(tuple_keys or re.findall(r"StaticStrings::(\w+)", values))
    return arrays


def _registered_module_attributes(
    create_body: str,
    source: str,
    static_strings: dict[str, str],
) -> set[str]:
    """Extract direct and array-backed attributes registered in ``create_module``."""
    variants = {variant for variant, _ in _iter_module_set_attr_calls(create_body)}
    arrays = _static_string_arrays(source)
    for loop in re.finditer(r"\bfor\b[^{};]*\bin\s+(\w+)\s*\{", create_body):
        body = _delimited_contents(create_body, create_body.find("{", loop.start()), "{", "}")
        if body is not None and "module.set_attr" in body:
            variants.update(arrays.get(loop.group(1), set()))
    return {static_strings[variant] for variant in variants if variant in static_strings}


def _module_type_bindings(
    create_body: str,
    static_strings: dict[str, str],
) -> dict[str, str]:
    """Map module-export names to their ``Type::<Variant>`` runtime type."""
    bindings: dict[str, str] = {}
    for variant, args in _iter_module_set_attr_calls(create_body):
        type_match = re.search(r"Builtins::Type\(Type::(\w+)\)", args)
        if type_match and variant in static_strings:
            bindings[static_strings[variant]] = type_match.group(1)
    return bindings


def _registered_modules(
    source: str,
    static_strings: dict[str, str],
) -> list[tuple[str, set[str], dict[str, str]]]:
    """Extract each module root plus its exports and runtime type bindings."""
    registered: list[tuple[str, set[str], dict[str, str]]] = []
    for create_body in _function_bodies(source, "create_module"):
        module_match = re.search(r"Module::new\(StaticStrings::(\w+)\)", create_body)
        if module_match is None:
            continue
        module_variant = module_match.group(1)
        module_name = static_strings.get(module_variant)
        if module_name is None:
            continue
        registered.append(
            (
                module_name,
                _registered_module_attributes(create_body, source, static_strings),
                _module_type_bindings(create_body, static_strings),
            )
        )
    return registered


def _pytrait_implementation_bodies(source: str) -> Iterator[str]:
    """Yield concrete ``PyTrait`` implementation bodies from a Rust source file."""
    pattern = re.compile(r"\bimpl(?:<[^{}]*>)?\s+PyTrait(?:<[^{}]*>)?\s+for\s+[^{}]*\{")
    for match in pattern.finditer(source):
        body = _delimited_contents(source, source.find("{", match.start()), "{", "}")
        if body is not None:
            yield body


def _runtime_type_variants(implementation: str, source: str) -> set[str]:
    """Resolve the ``Type`` variants served by one concrete ``PyTrait`` impl."""
    variants: set[str] = set()
    local_functions = dict(_iter_rust_function_bodies(source))
    for body in _function_bodies(implementation, "py_type"):
        variants.update(re.findall(r"\bType::(\w+)", body))
        for function_name in re.findall(r"\b([A-Za-z_]\w*)\s*\(", body):
            # Trait dispatchers call ``value.py_type(...)`` recursively. Looking
            # up another same-named function by filename would arbitrarily bind
            # the dispatcher to an unrelated implementation in that file.
            if function_name == "py_type":
                continue
            helper = local_functions.get(function_name)
            if helper is not None:
                variants.update(re.findall(r"\bType::(\w+)", helper))
    return variants


def _dispatch_static_variants(body: str) -> set[str]:
    """Extract StaticStrings used as an attribute/method dispatch selector."""
    variants: set[str] = set()
    selector = re.compile(r"\b(?:method\w*|attr\w*|ss)\b")

    for match in re.finditer(r"\bmatch\s+([^{};]+?)\s*\{", body):
        if not selector.search(match.group(1)):
            continue
        match_body = _delimited_contents(body, body.find("{", match.start()), "{", "}")
        if match_body is not None:
            variants.update(re.findall(r"StaticStrings::(\w+)", match_body))

    for match in re.finditer(r"\bmatches!\s*\(", body):
        contents = _delimited_contents(body, body.find("(", match.start()), "(", ")")
        if contents is None:
            continue
        first_argument = contents.split(",", 1)[0]
        if selector.search(first_argument):
            variants.update(re.findall(r"StaticStrings::(\w+)", contents))

    for line in body.splitlines():
        if selector.search(line) and "StaticStrings::" in line and re.search(r"\bif\b", line):
            variants.update(re.findall(r"StaticStrings::(\w+)", line))
    return variants


def _delegated_dispatch_calls(body: str) -> set[str]:
    """Find helper calls that receive a runtime attribute/method selector."""
    calls: set[str] = set()
    for match in re.finditer(r"\b([A-Za-z_]\w*)\s*\(", body):
        # Calls back into the PyTrait interface are dynamic dispatch, not a
        # concrete helper. Following their globally repeated names merges the
        # methods of every runtime type into forwarding enums such as
        # ``HeapReadOutput``.
        if match.group(1) in {"py_call_attr", "py_getattr"}:
            continue
        args = _delimited_contents(body, body.find("(", match.start()), "(", ")")
        if args is not None and re.search(r"\b(?:method\w*|attr\w*|ss)\b", args):
            calls.add(match.group(1))
    return calls


def _guarded_type_variants(guard: str | None, variants: set[str]) -> set[str]:
    """Resolve simple ``is_<type>()`` dispatch guards to runtime Type variants."""
    if guard is None:
        return set(variants)
    matches = re.findall(r"(!?)\s*is_([a-z_]+)\s*\(", guard)
    if not matches:
        return set(variants)

    selected: set[str] = set()
    for negated, predicate in matches:
        predicate_key = predicate.replace("_", "").lower()
        matching = {
            variant for variant in variants if variant.replace("_", "").lower() == predicate_key
        }
        if not matching:
            continue
        selected.update(variants - matching if negated else matching)
    return selected or set(variants)


def _attributes_from_dispatch(
    body: str,
    variants: set[str],
    static_strings: dict[str, str],
) -> dict[str, set[str]]:
    """Parse a dispatch body, preserving type-specific guarded match arms."""
    attributes = {variant: set[str]() for variant in variants}
    decisions: dict[tuple[str, str], bool] = {}
    selector = re.compile(r"\b(?:method\w*|attr\w*|ss)\b")
    arm_pattern = re.compile(
        r"(?P<pattern>(?:Some\()??StaticStrings::\w+\)?(?:\s*\|\s*(?:Some\()??StaticStrings::\w+\)?)*)(?:\s+if\s+(?P<guard>.*?))?\s*=>",
        re.DOTALL,
    )

    for match in re.finditer(r"\bmatch\s+([^{};]+?)\s*\{", body):
        if not selector.search(match.group(1)):
            continue
        match_body = _delimited_contents(body, body.find("{", match.start()), "{", "}")
        if match_body is None:
            continue
        arms = list(arm_pattern.finditer(match_body))
        for index, arm in enumerate(arms):
            arm_end = arms[index + 1].start() if index + 1 < len(arms) else len(match_body)
            arm_body = match_body[arm.end() : arm_end]
            supported = "not_implemented(" not in arm_body
            targets = _guarded_type_variants(arm.group("guard"), variants)
            for static_variant in re.findall(r"StaticStrings::(\w+)", arm.group("pattern")):
                name = static_strings.get(static_variant)
                if name is None:
                    continue
                for variant in targets:
                    decisions.setdefault((variant, name), supported)

        for arm in re.finditer(
            r"Some\(\w+\)\s+if\s+\w+\s*==\s*StaticStrings::(\w+)\s*=>",
            match_body,
        ):
            name = static_strings.get(arm.group(1))
            if name is not None:
                for variant in variants:
                    decisions.setdefault((variant, name), True)

    for match in re.finditer(r"\bmatches!\s*\(", body):
        contents = _delimited_contents(body, body.find("(", match.start()), "(", ")")
        if contents is None or not selector.search(contents.split(",", 1)[0]):
            continue
        for static_variant in re.findall(r"StaticStrings::(\w+)", contents):
            name = static_strings.get(static_variant)
            if name is not None:
                for variant in variants:
                    decisions.setdefault((variant, name), True)

    for line in body.splitlines():
        if "StaticStrings::" not in line or not selector.search(line) or "if" not in line:
            continue
        targets = _guarded_type_variants(line, variants)
        for static_variant in re.findall(r"StaticStrings::(\w+)", line):
            name = static_strings.get(static_variant)
            if name is not None:
                for variant in targets:
                    decisions.setdefault((variant, name), True)

    for (variant, name), supported in decisions.items():
        if supported:
            attributes[variant].add(name)
    return attributes


def _type_dispatch_attributes(
    implementation: str,
    function_index: dict[str, list[str]],
    static_strings: dict[str, str],
    variants: set[str],
) -> dict[str, set[str]]:
    """Extract direct and helper-delegated attributes for one ``PyTrait`` implementation."""
    pending = [
        body
        for name, body in _iter_rust_function_bodies(implementation)
        if name in {"py_call_attr", "py_getattr"}
    ]
    visited_bodies: set[str] = set()
    pending_helpers: set[str] = set()
    attributes = {variant: set[str]() for variant in variants}

    while pending:
        body = pending.pop()
        if body in visited_bodies:
            continue
        visited_bodies.add(body)
        for variant, names in _attributes_from_dispatch(body, variants, static_strings).items():
            attributes[variant].update(names)
        for helper in _delegated_dispatch_calls(body):
            pending_helpers.add(helper)
        while pending_helpers:
            helper = pending_helpers.pop()
            for helper_body in function_index.get(helper, []):
                if helper_body not in visited_bodies and "StaticStrings::" in helper_body:
                    pending.append(helper_body)

    return attributes


def _class_method_attributes(source: str, static_strings: dict[str, str]) -> dict[str, set[str]]:
    """Extract ``Type`` class-method dispatch (for example ``dict.fromkeys``)."""
    attributes: dict[str, set[str]] = {}
    for body in _function_bodies(source, "call_class_method"):
        pattern = re.compile(
            r"\(\s*Self::(\w+)\s*,\s*(\w+)\s*\)\s*if\s+\2\s*==\s*StaticStrings::(\w+)"
        )
        for match in pattern.finditer(body):
            # A type can deliberately reserve a classmethod name only to raise
            # NotImplementedError (for example Counter.fromkeys). It is not a
            # supported capability and must not be exposed.
            if "not_implemented" in body[match.end() : match.end() + 500]:
                continue
            name = static_strings.get(match.group(3))
            if name is not None:
                attributes.setdefault(match.group(1), set()).add(name)
    return attributes


# ══════════════════════════════════════════════════════════════════════
# Source bundle
# ══════════════════════════════════════════════════════════════════════


@dataclass
class _Sources:
    builtins: str
    modules: str
    types: str
    exceptions: str
    intern: str
    rust_files: dict[str, str]

    @classmethod
    def from_local(cls, root: Path) -> _Sources:
        rust_files = {
            p.relative_to(root).as_posix(): p.read_text(encoding="utf-8")
            for p in sorted((root / "crates").rglob("*.rs"))
        }

        def read_first(paths: tuple[str, ...]) -> str:
            for relative_path in paths:
                path = root / relative_path
                if path.exists():
                    return path.read_text(encoding="utf-8")
            raise FileNotFoundError(f"Monty source is missing all of: {', '.join(paths)}")

        return cls(
            builtins=read_first(_BUILTINS_RELS),
            modules=(root / _MODULES_REL).read_text(encoding="utf-8"),
            types=(root / _TYPES_REL).read_text(encoding="utf-8"),
            exceptions=read_first(_EXCEPTIONS_RELS),
            intern=(root / _INTERN_REL).read_text(encoding="utf-8"),
            rust_files=rust_files,
        )

    @classmethod
    def from_zip(cls, zf: zipfile.ZipFile, prefix: str) -> _Sources:
        def read(rel: str) -> str:
            return zf.read(prefix + rel).decode()

        crates_prefix = prefix + "crates/"
        rust_files = {
            item.filename[len(prefix) :]: zf.read(item.filename).decode()
            for item in zf.infolist()
            if item.filename.startswith(crates_prefix) and item.filename.endswith(".rs")
        }

        def read_first(paths: tuple[str, ...]) -> str:
            for relative_path in paths:
                source = rust_files.get(relative_path)
                if source is not None:
                    return source
            raise KeyError(f"Monty archive is missing all of: {', '.join(paths)}")

        return cls(
            builtins=read_first(_BUILTINS_RELS),
            modules=read(_MODULES_REL),
            types=read(_TYPES_REL),
            exceptions=read_first(_EXCEPTIONS_RELS),
            intern=read(_INTERN_REL),
            rust_files=rust_files,
        )


def _build_from_sources(src: _Sources) -> MontyCapabilities:
    """Build the complete statically discoverable capability graph."""
    static_strings = _parse_static_strings_map(_without_feature_gated_items(src.intern))
    builtin_type_variants = _parse_builtin_type_variants(_without_feature_gated_items(src.types))
    active_modules_source = _without_feature_gated_items(src.modules)
    active_files = {
        path: _without_feature_gated_items(source) for path, source in src.rust_files.items()
    }

    registered_modules: dict[str, set[str]] = {}
    registered_bindings: dict[str, dict[str, str]] = {}
    for source in active_files.values():
        for module_name, attributes, bindings in _registered_modules(source, static_strings):
            registered_modules.setdefault(module_name, set()).update(attributes)
            registered_bindings.setdefault(module_name, {}).update(bindings)

    # The standard-library dispatch is authoritative. A source file may exist
    # solely for a feature-gated module (``gc`` is currently test-only), and
    # must not become public merely because it has a create_module function.
    parsed_module_names = _parse_builtin_modules(active_modules_source, static_strings)
    module_names = parsed_module_names or set(registered_modules)
    module_attributes = {name: set(registered_modules.get(name, set())) for name in module_names}
    type_paths: dict[str, set[str]] = {
        variant: {name} for variant, name in builtin_type_variants.items()
    }
    for module_name in module_names:
        for export_name, type_variant in registered_bindings.get(module_name, {}).items():
            type_paths.setdefault(type_variant, set()).add(f"{module_name}.{export_name}")
    for module_name in module_names:
        module_attributes.setdefault(module_name, set())

    function_index: dict[str, list[str]] = {}
    for source in active_files.values():
        for name, body in _iter_rust_function_bodies(source):
            function_index.setdefault(name, []).append(body)

    type_variant_attributes: dict[str, set[str]] = _class_method_attributes(
        _without_feature_gated_items(src.types), static_strings
    )
    for source in active_files.values():
        for implementation in _pytrait_implementation_bodies(source):
            variants = _runtime_type_variants(implementation, source)
            # ``Type::Type`` is the shared metatype of every class object. A
            # concrete class implementation (such as a generated namedtuple)
            # must not make its class-specific attributes appear on ``type``.
            variants.discard("Type")
            if not variants:
                continue
            attributes_by_variant = _type_dispatch_attributes(
                implementation, function_index, static_strings, variants
            )
            for variant, attributes in attributes_by_variant.items():
                type_variant_attributes.setdefault(variant, set()).update(attributes)

    type_attributes: dict[str, frozenset[str]] = {}
    for type_variant, paths in type_paths.items():
        attribute_names = frozenset(type_variant_attributes.get(type_variant, set()))
        for path in paths:
            type_attributes[path] = attribute_names

    return MontyCapabilities(
        builtin_functions=frozenset(_parse_builtin_functions(src.builtins)),
        type_constructors=frozenset(builtin_type_variants.values()),
        exception_types=frozenset(_parse_exception_types(src.exceptions)),
        modules=frozenset(module_names),
        module_attributes={name: frozenset(values) for name, values in module_attributes.items()},
        type_attributes=type_attributes,
    )


# ══════════════════════════════════════════════════════════════════════
# Capability graph container
# ══════════════════════════════════════════════════════════════════════


@dataclass(frozen=True)
class MontyCapabilities:
    """The source-backed Python feature surface implemented by Monty.

    ``type_attributes`` uses canonical Python paths: bare paths for builtins
    (``str`` and ``dict``) and module-qualified paths for imported types
    (``pathlib.Path``, ``datetime.datetime``, and ``re.Pattern``).
    """

    builtin_functions: frozenset[str] = field(default_factory=frozenset)
    type_constructors: frozenset[str] = field(default_factory=frozenset)
    exception_types: frozenset[str] = field(default_factory=frozenset)
    modules: frozenset[str] = field(default_factory=frozenset)
    module_attributes: dict[str, frozenset[str]] = field(default_factory=dict)
    type_attributes: dict[str, frozenset[str]] = field(default_factory=dict)

    @classmethod
    def from_local(cls, monty_root: str | Path) -> MontyCapabilities:
        """Build capabilities from a local Monty checkout using the Rust extractor."""
        from ._native import _extract_local_json

        payload: object = json.loads(_extract_local_json(Path(monty_root)))
        if not isinstance(payload, dict):
            raise RuntimeError("Rust capability extractor returned a non-object payload")
        return cls.from_dict(payload)

    @classmethod
    def from_github(
        cls,
        url: str = _GITHUB_ZIP,
        *,
        branch: str = "main",
        only_released: bool = True,
    ) -> MontyCapabilities:
        """Download Monty and build capabilities from its Rust source in memory."""
        del branch  # Retained for backwards-compatible call signatures.
        from ._native import _extract_github_json

        payload: object = json.loads(_extract_github_json(url, only_released))
        if not isinstance(payload, dict):
            raise RuntimeError("Rust capability extractor returned a non-object payload")
        return cls.from_dict(payload)

    # ── Cache-backed class-level accessors ────────────────────────────

    @classmethod
    def _cached(cls, *, cache: bool = True, only_released: bool = True) -> MontyCapabilities:
        from .cache import get_capabilities

        return get_capabilities(
            cache="auto" if cache else "regenerate", only_released=only_released
        )

    @classmethod
    def get_modules(cls, *, cache: bool = True, only_released: bool = True) -> frozenset[str]:
        """Return importable Monty module names."""
        return cls._cached(cache=cache, only_released=only_released).modules

    @classmethod
    def get_builtins(cls, *, cache: bool = True, only_released: bool = True) -> frozenset[str]:
        """Return implemented builtin function names."""
        return cls._cached(cache=cache, only_released=only_released).builtin_functions

    @classmethod
    def get_types(cls, *, cache: bool = True, only_released: bool = True) -> frozenset[str]:
        """Return builtin type constructor names."""
        return cls._cached(cache=cache, only_released=only_released).type_constructors

    @classmethod
    def get_exception_types(
        cls, *, cache: bool = True, only_released: bool = True
    ) -> frozenset[str]:
        """Return implemented exception class names."""
        return cls._cached(cache=cache, only_released=only_released).exception_types

    @classmethod
    def get_attrs_of_module(
        cls,
        module: str,
        *,
        cache: bool = True,
        only_released: bool = True,
    ) -> frozenset[str]:
        """Return attributes registered directly on an importable module."""
        return cls._cached(cache=cache, only_released=only_released).get_attributes(module)

    @classmethod
    def get_attrs_of_type(
        cls,
        type_path: str,
        *,
        cache: bool = True,
        only_released: bool = True,
    ) -> frozenset[str]:
        """Return source-backed attributes of a builtin or imported runtime type.

        ``type_path`` is a canonical Python path such as ``str``, ``dict``,
        ``pathlib.Path``, or ``datetime.datetime``.
        """
        caps = cls._cached(cache=cache, only_released=only_released)
        return caps.type_attributes.get(type_path, frozenset())

    # ── Capability queries ───────────────────────────────────────────

    def get_attributes(self, path: str) -> frozenset[str]:
        """Return attributes registered for a module or runtime type path."""
        if path in self.module_attributes:
            return self.module_attributes[path]
        return self.type_attributes.get(path, frozenset())

    def supports_path(self, path: str) -> bool:
        """Return whether a canonical module/type/attribute path is known to Monty."""
        if path in self.modules or path in self.type_attributes:
            return True
        parent, separator, attribute = path.rpartition(".")
        return bool(separator and attribute in self.get_attributes(parent))

    # ── JSON serialisation ────────────────────────────────────────────

    def to_dict(self) -> dict[str, Any]:
        """Serialise capabilities to a deterministic JSON-safe dictionary."""
        return {
            "builtin_functions": sorted(self.builtin_functions),
            "type_constructors": sorted(self.type_constructors),
            "exception_types": sorted(self.exception_types),
            "modules": sorted(self.modules),
            "module_attributes": {
                key: sorted(value) for key, value in sorted(self.module_attributes.items())
            },
            "type_attributes": {
                key: sorted(value) for key, value in sorted(self.type_attributes.items())
            },
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MontyCapabilities:
        """Restore capabilities from a JSON-safe dictionary."""
        return cls(
            builtin_functions=frozenset(data.get("builtin_functions", [])),
            type_constructors=frozenset(data.get("type_constructors", [])),
            exception_types=frozenset(data.get("exception_types", [])),
            modules=frozenset(data.get("modules", [])),
            module_attributes={
                key: frozenset(value) for key, value in data.get("module_attributes", {}).items()
            },
            type_attributes={
                key: frozenset(value) for key, value in data.get("type_attributes", {}).items()
            },
        )

    # ── Human and prompt output ───────────────────────────────────────

    def summary(self) -> str:
        """Return a human-readable summary of the complete capability graph."""
        lines = ["Monty Sandbox Capabilities", "=" * 40]
        lines.append(f"\nBuiltin Functions ({len(self.builtin_functions)}):")
        lines.extend(f"  - {name}" for name in sorted(self.builtin_functions))
        lines.append(f"\nType Constructors ({len(self.type_constructors)}):")
        lines.extend(f"  - {name}" for name in sorted(self.type_constructors))
        lines.append(f"\nException Types ({len(self.exception_types)}):")
        lines.extend(f"  - {name}" for name in sorted(self.exception_types))
        lines.append(f"\nModules ({len(self.modules)}):")
        for name in sorted(self.modules):
            attrs = self.get_attributes(name)
            lines.append(f"  - {name}" + (f": {', '.join(sorted(attrs))}" if attrs else ""))
        lines.append(f"\nRuntime Types ({len(self.type_attributes)}):")
        for path, attrs in sorted(self.type_attributes.items()):
            lines.append(f"  - {path}" + (f": {', '.join(sorted(attrs))}" if attrs else ""))
        return "\n".join(lines)

    def to_prompt_context(self) -> str:
        """Return a structured prompt block describing Monty's capabilities."""
        lines = [
            "## Monty Sandbox — Supported Python Features",
            "",
            "Only the source-backed features below are available in the Monty sandbox.",
            "",
            "### Built-in functions",
            ", ".join(sorted(self.builtin_functions)),
            "",
            "### Type constructors",
            ", ".join(sorted(self.type_constructors)),
            "",
            "### Exception types",
            ", ".join(sorted(self.exception_types)),
            "",
            "### Supported modules",
        ]
        for module in sorted(self.modules):
            attrs = self.get_attributes(module)
            lines.append(f"- `{module}`: " + (", ".join(sorted(attrs)) if attrs else "no exports"))
        lines.extend(["", "### Runtime type attributes"])
        for path, attrs in sorted(self.type_attributes.items()):
            description = ", ".join(sorted(attrs)) if attrs else "no known attributes"
            lines.append(f"- `{path}`: {description}")
        lines.extend(
            [
                "",
                "### Hard constraints",
                "- Do not use a module, attribute, builtin, or type absent from this list.",
            ]
        )
        return "\n".join(lines)
