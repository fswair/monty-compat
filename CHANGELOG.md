# Changelog

All notable changes to `monty-compat` are documented in this file.

## [0.5.0] - 2026-08-01

### Added

- Added the native Rust `monty-compat` lowering engine and `monty-lower` CLI,
  with bounded exact-source caching, checked UTF-8 edit ranges, deterministic
  helper names, and no production `unsafe` code.
- Added the native Rust `monty-compat-extract` crate and `monty-extract` CLI for
  source-backed discovery of builtins, exception types, modules, module
  attributes, constructors, and runtime-type attributes such as
  `pathlib.Path.is_dir`.
- Added exact-release behavioral discovery against Monty 0.0.19, including
  killable workers, CPython comparison, deterministic `pysource-codegen`
  corpora, `pysource-minimize` failure reduction, and atomic manifest output.
- Added the reviewed Monty 0.0.19 capability manifest. It records 160 directly
  supported behavioral features, four automatic lowering seams, 78 contextual
  lowering seams, and 27 deliberately non-lowerable seams.
- Added evidence-backed lowering for safe cases across pattern matching,
  decorators, complex `for` and `with` targets, class and dataclass forms,
  formatting, protocols, lazy builtins, comprehensions, dictionary union,
  deletion, assertions, and selected async behavior.
- Added Rust, Python, differential, malformed-input, cache, golden-output, and
  fail-closed diagnostic test suites.
- Added reproducible p50, median, and p99 benchmark reports for transpilation,
  cache hits, extraction, discovery, probes, and Monty execution.
- Added Python 3.10+ ABI3 wheels, PyO3 type stubs, Rust crate packaging, cargo-deny
  release gates, PyPI trusted publishing, and crates.io publishing automation.
- Added a Zensical documentation site and GitHub Pages deployment, including the
  validated manifest channel used by explicit `release="latest"` resolution.
- Added detailed examples, maintainer documentation, and a reusable
  `.agents/skills/monty-compat` skill.

### Changed

- Replaced the old static compatibility-checking path with the fail-closed
  `transpiler(code, release="verified")` Python API.
- Made `verified` the default release selector. It is offline and resolves to
  the newest manifest compiled into the wheel, currently Monty 0.0.19.
- Added explicit `latest` resolution through a bounded HTTPS channel with
  SHA-256 verification, release identity checks, lowering-engine compatibility
  checks, known-feature validation, and process-local caching. It never silently
  falls back to `verified`.
- Kept exact bundled selectors such as `0.0.19` and `v0.0.19` deterministic and
  offline.
- Moved extraction, lowering, cache, probe execution, minimization verdicts, and
  exact Monty execution control into Rust where practical. Python remains the
  generator and CPython-oracle boundary for `pysource-codegen` and
  `pysource-minimize`.
- Expanded capability serialization and prompt context with nested module and
  runtime-type attribute surfaces.

### Removed

- Removed the misleading public `check_code` compatibility API and related
  top-level checker aliases. Unsupported or semantically unsafe input is now an
  explicit `TranspilationError` rather than an optimistic boolean result.

### Security

- Added bounded downloads, archive entry/path/expanded-size validation, exact
  runtime fingerprints, subprocess timeouts, hash-checked remote manifests,
  poisoned-lock recovery, panic-resistant arbitrary-input tests, and fail-closed
  handling for `needs_review` and `not_lowerable` diagnostics.
- Added locked dependency graphs and cargo-deny checks for both the publishable
  workspace and the isolated exact-Monty discovery workspace.

[0.5.0]: https://github.com/fswair/monty-compat/releases/tag/v0.5.0
