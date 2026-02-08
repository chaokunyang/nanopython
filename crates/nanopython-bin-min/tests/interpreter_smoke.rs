use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("failed to resolve workspace root")
}

fn run_nanopython(args: &[&str]) -> Output {
    let exe = env!("CARGO_BIN_EXE_nanopython");
    Command::new(exe)
        .current_dir(workspace_root())
        .args(args)
        .output()
        .expect("failed to run nanopython")
}

fn run_nanopython_with_pythonpath(args: &[&str], pythonpath: &Path) -> Output {
    let exe = env!("CARGO_BIN_EXE_nanopython");
    Command::new(exe)
        .current_dir(workspace_root())
        .env("PYTHONPATH", pythonpath)
        .args(args)
        .output()
        .expect("failed to run nanopython with PYTHONPATH")
}

#[test]
fn supports_required_core_features() {
    let code = r#"
from dataclasses import dataclass
from enum import Enum, auto
import gc
import os

@dataclass
class Item:
    value: int

class Kind(Enum):
    A = auto()
    B = auto()

def gen():
    with open("nanopython_test_tmp.txt", "w", encoding="utf-8") as f:
        f.write("42")
    with open("nanopython_test_tmp.txt", "r", encoding="utf-8") as f:
        v = f.read()
    os.remove("nanopython_test_tmp.txt")
    yield v

obj = Item(7)
assert obj.value == 7
assert Kind.A.name == "A"
assert list(gen()) == ["42"]

xs = [1, 2, 3]
d = {"x": 1}
s = {1, 2, 3}
assert len(xs) == 3 and d["x"] == 1 and 2 in s

total = 0
for i in range(6):
    if i % 2 == 0:
        total += i

j = 0
while j < 3:
    j += 1

assert total == 6
assert j == 3
assert isinstance(gc.collect(), int)
assert isinstance(gc.get_threshold(), tuple)

print("OK")
"#;

    let out = run_nanopython(&["-c", code]);
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "OK");
}

#[test]
fn blocks_unsupported_modules() {
    let out = run_nanopython(&["-c", "import threading"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("disabled in nanopython min profile"),
        "{stderr}"
    );
}

#[test]
fn runs_fory_compiler_module() {
    let root = workspace_root();
    let compiler_root = root.join("fory-main/compiler");
    let output_dir = root.join("nanopython-test-out");

    let help = run_nanopython_with_pythonpath(&["-m", "fory_compiler", "--help"], &compiler_root);
    assert!(
        help.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );

    if output_dir.exists() {
        std::fs::remove_dir_all(&output_dir).expect("failed to remove stale output dir");
    }

    let compile = run_nanopython_with_pythonpath(
        &[
            "-m",
            "fory_compiler",
            "fory-main/compiler/examples/demo.fdl",
            "--lang",
            "java,python,cpp,rust,go",
            "--output",
            "nanopython-test-out",
        ],
        &compiler_root,
    );
    assert!(
        compile.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    assert!(output_dir.join("java/demo/Color.java").exists());
    assert!(output_dir.join("python/demo.py").exists());
    assert!(output_dir.join("cpp/demo.h").exists());
    assert!(output_dir.join("rust/demo.rs").exists());
    assert!(output_dir.join("go/demo/demo.go").exists());

    std::fs::remove_dir_all(&output_dir).expect("failed to cleanup compiler output dir");
}
