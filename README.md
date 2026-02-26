# nanopython

Minimal Rust-based Python interpreter implementation (from scratch, no RustPython dependency).

## Design

- The `nanopy` binary is implemented in Rust.
- No host CPython embedding.
- No RustPython crates.
- Tree-walk interpreter with custom lexer/parser/runtime.

## Current phase (phase 1)

Implemented and working:

- script and `-c` execution
- package directory execution (`path/to/pkg` with `__main__.py`)
- imports (custom modules + builtin `os`, `sys`, `dataclasses`, `enum`)
- lists / dicts / sets
- if / while / for / range
- functions, classes, methods
- with statement for file objects
- yield / basic generators
- list/set/dict/generator comprehensions (single-level)
- try/except/finally and raise (minimal behavior)

Still incomplete (next phases):

- full Python 3.8 compatibility
- enough stdlib surface to run all of `fory-main/compiler/fory_compiler`
- strict output consistency vs CPython for compiler codegen
- full dataclass/enum semantics
- final optimization pass for performance and feature completeness

## Build

```bash
cargo build --release
```

Binary path and size:

```bash
stat -f '%N %z bytes' target/release/nanopy
```
