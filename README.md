# nanopython

Rust-based Python interpreter workspace with pluggable crate composition.

## Workspace crates

- `nanopython-core`: shared errors and capability model.
- `nanopython-plugin-api`: plugin traits and capability validation.
- `nanopython-vm`: interpreter runner on top of `rustpython-vm`.
- `nanopython-stdlib-core`: min-profile stdlib policy plugin + minimal native modules (`_dis`, `gc`, `math`).
- `nanopython-feature-enum-lite`: default enum capability provider.
- `nanopython-feature-dataclass-lite`: default dataclass capability provider.
- `nanopython-selfvm`: self-hosted interpreter backend (work in progress).
- `nanopython-bin-self`: `nanopython-self` binary using only self-hosted backend.
- `nanopython-bin-min`: `nanopython` binary.
- `nanopython-bin-extended`: `nanopython-extended` binary.

## Build

```bash
cargo build -p nanopython-bin-min --release
cargo build -p nanopython-bin-self --release
```

## Run

```bash
# help
./target/release/nanopython --help

# command
./target/release/nanopython -c "print('hello')"

# module
PYTHONPATH=$PWD/fory-main/compiler ./target/release/nanopython -m fory_compiler --help

# self-hosted backend binary
./target/release/nanopython-self -c "x = 1
y = x + 2
print(y)"
```

## Validation commands used

```bash
cargo test
cargo test -p nanopython-bin-min
cargo test -p nanopython-selfvm
PYTHONPATH=$PWD/fory-main/compiler ./target/release/nanopython -m fory_compiler --help
PYTHONPATH=$PWD/fory-main/compiler ./target/release/nanopython -m fory_compiler \
  fory-main/compiler/examples/demo.fdl \
  --lang java,python,cpp,rust,go \
  --output nanopy-out-release2
```

## Current release size

- `target/release/nanopython`: `4,708,512` bytes on arm64 macOS.
- `target/release/nanopython-self`: `369,744` bytes on arm64 macOS.
