# NanoPython Design (Implemented)

## 1. Scope And Objectives

NanoPython is a Rust-based Python interpreter workspace focused on:

- Python 3.8-compatible execution for compiler workloads.
- Running `fory-main/compiler/fory_compiler` end-to-end.
- A pluggable, multi-crate architecture where capabilities are assembled by crate composition.
- Minimal default runtime surface with explicit unsupported modules.

Required runtime support in the min profile:

- `dataclass` and `enum`
- `os`, imports, file operations
- list/dict/set
- control flow (`if`, `while`, `for`, `range`)
- `with`, `yield`
- GC interface

Explicitly unsupported in min profile:

- `asyncio`
- `threading`
- `multiprocessing`
- `pickle`
- CPython extension loading
- dynamic metaclass behavior
- `socket`

## 2. Architecture Summary

NanoPython is a Cargo workspace with capability plugins over a VM backend.

- VM backend: `rustpython-vm` (`compiler` + `importlib` + `encodings`).
- Default stdlib path: `rustpython-pylib` `Lib/`.
- Native modules in min profile are intentionally small: `_dis`, `gc`, `math`.
- Unsupported modules are blocked by a `sitecustomize` import hook.

Key design decision:

- Keep parser/compiler/runtime correctness through RustPython backend.
- Keep feature composition and policy in NanoPython crates.

## 3. Workspace Crates

- `crates/nanopython-core`
  - shared errors and capability enum.
- `crates/nanopython-plugin-api`
  - plugin trait + registry + capability validation.
- `crates/nanopython-parser`
  - parser boundary crate (stable extension seam).
- `crates/nanopython-compiler`
  - compiler boundary crate (stable extension seam).
- `crates/nanopython-gc-rc`
  - RC/cycle-policy modeling helpers and tests.
- `crates/nanopython-import`
  - import policy helpers (blocked roots).
- `crates/nanopython-stdlib-core`
  - stdlib policy plugin + native modules (`_dis`, `gc`, `math`) + frozen `sitecustomize`.
- `crates/nanopython-feature-enum-lite`
  - enum provider capability plugin.
- `crates/nanopython-feature-dataclass-lite`
  - dataclass provider capability plugin.
- `crates/nanopython-vm`
  - CLI parsing + interpreter runner.
- `crates/nanopython-bin-min`
  - `nanopython` binary (default minimal composition).
- `crates/nanopython-bin-extended`
  - `nanopython-extended` binary (same base, extension target).

## 4. Plugin Model

Plugin contract (`nanopython-plugin-api`):

- `name()`
- `provides()`
- optional `requires()`
- optional `install(&mut VirtualMachine)`

Capability validation:

- Exactly one provider for singleton capabilities:
  - `EnumProvider`
  - `DataclassProvider`
- startup fails fast on missing/duplicate providers.

Current min binary composition:

- `stdlib-core` plugin
- `enum-lite` plugin
- `dataclass-lite` plugin

This keeps enum/dataclass replaceable by future dynamic crates without changing VM core.

## 5. Runtime Execution Flow

From `crates/nanopython-vm/src/lib.rs`:

1. Parse CLI (`-c`, `-m`, script path).
2. Build VM `Settings`:
   - append RustPython stdlib path (`rustpython_pylib::LIB_PATH`)
   - merge `RUSTPYTHONPATH` and `PYTHONPATH`
3. Validate plugin registry.
4. Initialize interpreter and install plugins.
5. Initialize `__main__`.
6. Import `site` (which loads frozen `sitecustomize` policy hook).
7. Execute command/module/script.

## 6. Language And Feature Coverage

Provided by backend + min plugins:

- Python 3.8 syntax and semantics needed by `fory_compiler`.
- `with` statement support.
- `yield` / generator support.
- import/module execution (`-m`).
- file operations via built-in I/O + stdlib modules.
- dataclass/enum behavior via stdlib runtime and capability providers.

Blocked by policy hook in min profile:

- `asyncio`
- `threading`
- `multiprocessing`
- `pickle`
- `socket`

## 7. GC Design

GC behavior target is RC + cycle-safe behavior.

- Object lifetime and cycle handling are provided by RustPython object runtime.
- `gc` module is provided in min profile with core API surface:
  - `collect`, `isenabled`, `enable`, `disable`
  - `get_count`, `get_threshold`, `set_threshold`
  - `get_debug`, `set_debug`
  - `get_objects`, `get_referents`, `get_referrers`, `get_stats`, `is_tracked`

`nanopython-gc-rc` contains scheduling policy utilities and tests for periodic cycle-check triggers.

## 8. Validation And Test Strategy

Automated tests:

- Unit tests across core/plugin/import/gc crates.
- Integration tests in `crates/nanopython-bin-min/tests/interpreter_smoke.rs`:
  - required core features (`dataclass`, `enum`, `with`, `yield`, file/os/import/control flow, `gc`)
  - blocked module behavior (`threading`)
  - `fory_compiler` module execution and output generation

Manual/runtime checks used:

- `cargo test`
- `PYTHONPATH=$PWD/fory-main/compiler target/release/nanopython -m fory_compiler --help`
- compile demo FDL to java/python/cpp/rust/go outputs

## 9. Binary Size

Current measured release size (arm64 macOS):

- `target/release/nanopython`: `4,708,512` bytes
- `target/release/nanopython-self`: `369,744` bytes

Status versus strict target:

- Target: `< 2,000,000` bytes
- Current: not yet met

What already reduced size materially:

- Removed dependency on `rustpython-stdlib` (large native module surface).
- Kept only minimal native modules in NanoPython.
- aggressive release profile:
  - `opt-level = "z"`
  - `lto = "fat"`
  - `codegen-units = 1`
  - `panic = "abort"`
  - `strip = "symbols"`

Remaining dominant footprint:

- RustPython VM/compiler core and parser/runtime tables required for 3.8 execution + imports.

## 11. Self-Hosted Backend Track (No RustPython Path)

A new self-hosted backend has been added as the migration path away from RustPython:

- `crates/nanopython-parser`: custom AST parser for a Python-like subset.
- `crates/nanopython-selfvm`: custom evaluator/runtime (control flow, functions, classes, `with`, `yield`, imports, containers, file I/O, minimal builtins/modules).
- `crates/nanopython-bin-self`: standalone `nanopython-self` binary using only self-hosted crates.

Current validated capabilities on self backend:

- assignments, expressions, `if`/`while`/`for` + `range`
- functions + `return`
- generators with `yield` (collection-style semantics)
- class definitions + method/class attribute access
- decorators (minimal behavior) and basic `dataclass` decorator passthrough
- `with` + file operations (`open/read/write/close`)
- containers: list/dict/set + indexing
- basic imports (`import`, `from ... import ...`) for builtin and file modules

Current gap versus full replacement target:

- self backend does not yet run `fory_compiler` end-to-end.
- stdlib/module coverage (e.g. `argparse`, `pathlib`, `typing`, deeper object/exception semantics) is incomplete.

Migration rule:

- maintain existing passing tests on `nanopython` while expanding self-backend conformance.
- once self backend passes `fory_compiler` integration and required feature tests, switch default binary to self backend and remove RustPython dependency chain.

## 10. Path To `<2MB` (Required Follow-up)

To meet the `<2MB` hard gate while preserving current functionality, the remaining work must be root-cause size reduction in VM core, not policy-level trimming:

1. Fork and minimize `rustpython-vm` module/dependency surface for NanoPython profile.
2. Remove or gate heavy internal modules not needed by `fory_compiler` workload.
3. Re-measure and keep full interpreter + `fory_compiler` integration tests green after each cut.
4. Enforce size gate in CI once target is reached.

This is the next mandatory engineering phase to close the remaining size constraint.
