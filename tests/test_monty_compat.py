"""Source-shaped tests for Monty capability extraction and graph queries."""

from __future__ import annotations

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from io import BytesIO
from pathlib import Path
from threading import Thread
from zipfile import ZIP_DEFLATED, ZipFile

import pytest

from monty_compat import MontyCapabilities
from monty_compat._native import ExtractionError, _extract_archive_json, _extract_local_json
from monty_compat.cache import load_cache, save_cache
from monty_compat.capabilities import _build_from_sources, _Sources


def _write_monty_source(root: Path, relative_path: str, contents: str) -> None:
    path = root / "crates" / "monty" / "src" / relative_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents, encoding="utf-8")


def _monty_root(tmp_path: Path) -> Path:
    root = tmp_path / "monty"
    _write_monty_source(
        root,
        "intern.rs",
        """
        #[strum(serialize_all = "snake_case")]
        pub enum StaticStrings {
            Pathlib,
            #[strum(serialize = "Path")]
            PathClass,
            IsDir,
            Exists,
            IsAbsolute,
            Joinpath,
            Name,
            StrModule,
            Upper,
            Collections,
            #[strum(serialize = "Counter")]
            Counter,
            MostCommon,
            Get,
            Fromkeys,
            Looped,
            Run,
            Stop,
            DefaultFactory,
            Add,
            Union,
            Sys,
            Gc,
            Setrecursionlimit,
        }
        """,
    )
    _write_monty_source(
        root,
        "builtins/mod.rs",
        """
        #[strum(serialize_all = "lowercase")]
        pub enum BuiltinsFunctions {
            Abs,
            Print,
            // Eval,
        }
        """,
    )
    _write_monty_source(
        root,
        "exception_private.rs",
        """
        pub(crate) enum ExcType {
            ValueError,
            TypeError,
        }
        """,
    )
    _write_monty_source(
        root,
        "modules/mod.rs",
        """
        pub(crate) enum StandardLib { Pathlib, StrModule, Collections, Looped, Sys, Gc }
        fn from_string_id() {
            match value {
                StaticStrings::Pathlib => Some(Self::Pathlib),
                StaticStrings::StrModule => Some(Self::StrModule),
                StaticStrings::Collections => Some(Self::Collections),
                StaticStrings::Looped => Some(Self::Looped),
                StaticStrings::Sys => Some(Self::Sys),
                #[cfg(feature = "test-hooks")]
                StaticStrings::Gc => Some(Self::Gc),
            }
        }
        """,
    )
    _write_monty_source(
        root,
        "types/type.rs",
        """
        impl Type {
            pub fn from_builtin_name(name: &str) -> Option<Self> {
                match name {
                    "str" => Some(Self::Str),
                    "int" => Some(Self::Int),
                    "dict" => Some(Self::Dict),
                    "set" => Some(Self::Set),
                    "frozenset" => Some(Self::FrozenSet),
                    _ => None,
                }
            }

            fn call_class_method(self, method: StringId) {
                match (self, method) {
                    (Self::Dict, m) if m == StaticStrings::Fromkeys => Ok(()),
                    (Self::Counter, m) if m == StaticStrings::Fromkeys => {
                        Err(ExcType::not_implemented("Counter.fromkeys"))
                    }
                }
            }
        }
        """,
    )
    _write_monty_source(
        root,
        "modules/pathlib.rs",
        """
        fn create_module() {
            let mut module = Module::new(StaticStrings::Pathlib);
            module.set_attr(
                StaticStrings::PathClass,
                Value::Builtin(Builtins::Type(Type::Path)),
                vm,
            );
        }
        """,
    )
    _write_monty_source(
        root,
        "modules/str_module.rs",
        """
        fn create_module() {
            let mut module = Module::new(StaticStrings::StrModule);
        }
        """,
    )
    _write_monty_source(
        root,
        "modules/sys.rs",
        """
        fn create_module() {
            let mut module = Module::new(StaticStrings::Sys);
            #[cfg(feature = "test-hooks")]
            module.set_attr(StaticStrings::Setrecursionlimit, Value::Function, vm);
        }
        """,
    )
    _write_monty_source(
        root,
        "modules/gc.rs",
        """
        fn create_module() {
            let mut module = Module::new(StaticStrings::Gc);
            module.set_attr(StaticStrings::Run, Value::Function, vm);
        }
        """,
    )
    _write_monty_source(
        root,
        "modules/collections/mod.rs",
        """
        fn create_module() {
            let mut module = Module::new(StaticStrings::Collections);
            module.set_attr(
                StaticStrings::Counter,
                Value::Builtin(Builtins::Type(Type::Counter)),
                vm,
            );
        }
        """,
    )
    _write_monty_source(
        root,
        "modules/looped/mod.rs",
        """
        const FUNCTIONS: &[(StaticStrings, Function)] = &[
            (StaticStrings::Run, Function::Run),
            (StaticStrings::Stop, Function::Stop),
        ];

        fn create_module() {
            let mut module = Module::new(StaticStrings::Looped);
            for (name, function) in FUNCTIONS {
                module.set_attr(*name, Value::ModuleFunction(*function), vm);
            }
        }
        """,
    )
    _write_monty_source(
        root,
        "types/path.rs",
        """
        impl<'h> PyTrait<'h> for HeapRead<'h, Path> {
            fn py_type(&self) -> Type { Type::Path }
            fn py_call_attr(&mut self, attr: &EitherStr) {
                let method = attr.static_string();
                if is_path_os_method(method) { return; }
                match method {
                    StaticStrings::IsAbsolute | StaticStrings::Joinpath => Ok(()),
                    _ => Err(()),
                }
            }
            fn py_getattr(&self, attr: &EitherStr) {
                match attr.static_string() {
                    Some(StaticStrings::Name) => Ok(()),
                    _ => Err(()),
                }
            }
        }
        """,
    )
    _write_monty_source(
        root,
        "heap_dispatch.rs",
        """
        impl<'h> PyTrait<'h> for HeapReadOutput<'h> {
            fn py_type(&self) -> Type { Type::Int }
            fn py_call_attr(&mut self, attr: &EitherStr) {
                match self {
                    Self::Path(path) => path.py_call_attr(attr),
                    _ => Err(()),
                }
            }
        }
        """,
    )
    _write_monty_source(
        root,
        "os_dispatch.rs",
        """
        fn is_path_os_method(method: StaticStrings) -> bool {
            matches!(method, StaticStrings::Exists | StaticStrings::IsDir)
        }
        """,
    )
    _write_monty_source(
        root,
        "types/str.rs",
        """
        impl<'h> PyTrait<'h> for HeapRead<'h, Str> {
            fn py_type(&self) -> Type { Type::Str }
            fn py_call_attr(&mut self, attr: &EitherStr) {
                call_str_method(attr.static_string())
            }
        }
        fn call_str_method(method: StaticStrings) {
            match method { StaticStrings::Upper => Ok(()), _ => Err(()) }
        }
        """,
    )
    _write_monty_source(
        root,
        "types/dict.rs",
        """
        impl Dict {
            fn kind_type(&self) -> Type {
                match self.kind {
                    DictKind::Plain => Type::Dict,
                    DictKind::Counter => Type::Counter,
                }
            }
        }
        impl<'h> PyTrait<'h> for HeapRead<'h, Dict> {
            fn py_type(&self) -> Type { self.get().kind_type() }
            fn py_call_attr(&mut self, attr: &EitherStr) {
                let method = attr.static_string();
                match method {
                    StaticStrings::MostCommon if self.get().is_counter() => Ok(()),
                    StaticStrings::Fromkeys if self.get().is_counter() => {
                        Err(ExcType::not_implemented("Counter.fromkeys"))
                    }
                    StaticStrings::Get | StaticStrings::Fromkeys => Ok(()),
                    _ => Err(()),
                }
            }
        }
        """,
    )
    _write_monty_source(
        root,
        "types/set.rs",
        """
        impl<'h> PyTrait<'h> for HeapRead<'h, Set> {
            fn py_type(&self) -> Type { Type::Set }
            fn py_call_attr(&mut self, attr: &EitherStr) {
                match attr.static_string() {
                    Some(StaticStrings::Add) | Some(StaticStrings::Union) => Ok(()),
                    _ => Err(()),
                }
            }
        }
        impl<'h> PyTrait<'h> for HeapRead<'h, FrozenSet> {
            fn py_type(&self) -> Type { Type::FrozenSet }
            fn py_call_attr(&mut self, attr: &EitherStr) {
                match attr.static_string() {
                    Some(StaticStrings::Union) => Ok(()),
                    _ => Err(()),
                }
            }
        }
        """,
    )
    return root


def _monty_archive(root: Path, *, prefix: str = "monty-test") -> bytes:
    output = BytesIO()
    with ZipFile(output, "w", compression=ZIP_DEFLATED) as archive:
        for path in sorted(root.rglob("*.rs")):
            archive.write(path, f"{prefix}/{path.relative_to(root).as_posix()}")
    return output.getvalue()


def test_from_local_extracts_every_supported_node_shape(tmp_path: Path) -> None:
    caps = MontyCapabilities.from_local(_monty_root(tmp_path))

    assert caps.builtin_functions == frozenset({"abs", "print"})
    assert caps.type_constructors == frozenset({"dict", "frozenset", "int", "set", "str"})
    assert caps.exception_types == frozenset({"TypeError", "ValueError"})
    assert caps.modules == frozenset({"collections", "looped", "pathlib", "str_module", "sys"})
    assert "gc" not in caps.modules
    assert caps.get_attributes("sys") == frozenset()
    assert "setrecursionlimit" not in caps.get_attributes("sys")
    assert caps.get_attributes("looped") == frozenset({"run", "stop"})
    assert caps.get_attributes("pathlib") == frozenset({"Path"})

    assert {"exists", "is_dir", "is_absolute", "joinpath", "name"} <= caps.get_attributes(
        "pathlib.Path"
    )
    assert caps.get_attributes("str") == frozenset({"upper"})
    assert caps.get_attributes("int") == frozenset()
    assert caps.get_attributes("dict") == frozenset({"fromkeys", "get"})
    assert caps.get_attributes("collections.Counter") == frozenset({"get", "most_common"})
    assert "fromkeys" not in caps.get_attributes("collections.Counter")
    assert caps.get_attributes("set") == frozenset({"add", "union"})
    assert caps.get_attributes("frozenset") == frozenset({"union"})


def test_rust_local_extractor_matches_python_oracle(tmp_path: Path) -> None:
    root = _monty_root(tmp_path)
    python_graph = _build_from_sources(_Sources.from_local(root)).to_dict()
    rust_graph = json.loads(_extract_local_json(root))

    assert rust_graph == python_graph
    assert MontyCapabilities.from_local(root).to_dict() == python_graph


def test_rust_archive_extractor_matches_local_and_python_oracle(tmp_path: Path) -> None:
    root = _monty_root(tmp_path)
    expected = _build_from_sources(_Sources.from_local(root)).to_dict()

    assert json.loads(_extract_archive_json(_monty_archive(root))) == expected
    assert json.loads(_extract_archive_json(_monty_archive(root))) == json.loads(
        _extract_local_json(root)
    )


def test_from_github_uses_native_http_and_archive_extractor(tmp_path: Path) -> None:
    root = _monty_root(tmp_path)
    archive = _monty_archive(root)

    class ArchiveHandler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802
            self.send_response(200)
            self.send_header("Content-Type", "application/zip")
            self.send_header("Content-Length", str(len(archive)))
            self.end_headers()
            self.wfile.write(archive)

        def log_message(self, format: str, *args: object) -> None:
            del format, args

    server = ThreadingHTTPServer(("127.0.0.1", 0), ArchiveHandler)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        url = f"http://127.0.0.1:{server.server_port}/monty.zip"
        caps = MontyCapabilities.from_github(url, only_released=False)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)

    assert not thread.is_alive()
    assert caps.to_dict() == MontyCapabilities.from_local(root).to_dict()


@pytest.mark.parametrize("name", ["../escape.rs", "/absolute.rs", "root/../escape.rs"])
def test_rust_archive_extractor_rejects_unsafe_paths(name: str) -> None:
    output = BytesIO()
    with ZipFile(output, "w") as archive:
        archive.writestr(name, "pub fn escape() {}")

    with pytest.raises(ExtractionError, match="unsafe entry path"):
        _extract_archive_json(output.getvalue())


def test_rust_archive_extractor_rejects_duplicate_paths() -> None:
    output = BytesIO()
    with ZipFile(output, "w") as archive:
        archive.writestr("root/crates/duplicate.rs", "first")
        with pytest.warns(UserWarning, match="Duplicate name"):
            archive.writestr("root/crates/duplicate.rs", "second")

    with pytest.raises(ExtractionError, match="duplicate entry path"):
        _extract_archive_json(output.getvalue())


def test_rust_archive_extractor_rejects_invalid_zip() -> None:
    with pytest.raises(ExtractionError, match="ZIP archive"):
        _extract_archive_json(b"not a zip")


def test_queries_and_serialization_use_type_capabilities(tmp_path: Path) -> None:
    caps = MontyCapabilities.from_local(_monty_root(tmp_path))
    restored = MontyCapabilities.from_dict(caps.to_dict())

    assert restored == caps
    assert caps.supports_path("pathlib.Path.is_dir")
    assert not caps.supports_path("pathlib.Path.not_real")
    assert MontyCapabilities.get_attrs_of_type.__doc__


def test_cache_round_trip_preserves_type_attributes(tmp_path: Path) -> None:
    caps = MontyCapabilities.from_local(_monty_root(tmp_path))
    cache_dir = tmp_path / "cache"

    save_cache(caps, "fixture", cache_dir=cache_dir)
    loaded = load_cache("fixture", cache_dir=cache_dir)

    assert loaded == caps
    assert loaded is not None
    assert "is_dir" in loaded.get_attributes("pathlib.Path")
