import os
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from typing import List, Optional


REPO_ROOT = Path(__file__).resolve().parents[1]


class NanopyFeatureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        env_bin = os.environ.get("NANOPY_BIN")
        cls.nanopy_bin = Path(env_bin) if env_bin else REPO_ROOT / "target" / "release" / "nanopy"
        if not cls.nanopy_bin.exists():
            subprocess.run(
                ["cargo", "build", "--release", "--bin", "nanopy"],
                cwd=REPO_ROOT,
                check=True,
                text=True,
            )
        cls.nanopy_bin = cls.nanopy_bin.resolve()
        if not cls.nanopy_bin.exists():
            raise RuntimeError(f"nanopy binary not found: {cls.nanopy_bin}")

    def run_nanopy(self, *args: str, cwd: Optional[Path] = None, pythonpath: Optional[List[Path]] = None):
        env = os.environ.copy()
        if pythonpath is not None:
            env["PYTHONPATH"] = os.pathsep.join(str(p) for p in pythonpath)
        return subprocess.run(
            [str(self.nanopy_bin), *args],
            cwd=str(cwd or REPO_ROOT),
            env=env,
            capture_output=True,
            text=True,
        )

    def run_nanopy_code(self, code: str, cwd: Optional[Path] = None, pythonpath: Optional[List[Path]] = None):
        code = textwrap.dedent(code).strip() + "\n"
        return self.run_nanopy("-c", code, cwd=cwd, pythonpath=pythonpath)

    def assert_ok(self, proc: subprocess.CompletedProcess[str]) -> None:
        self.assertEqual(
            proc.returncode,
            0,
            msg=f"expected success\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}",
        )

    def assert_fail(self, proc: subprocess.CompletedProcess[str]) -> None:
        self.assertNotEqual(
            proc.returncode,
            0,
            msg=f"expected failure\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}",
        )

    def test_binary_size_under_2mb(self):
        size = self.nanopy_bin.stat().st_size
        self.assertLess(size, 2_000_000, f"binary too large: {size} bytes")

    def test_c_mode_runs_basic_code(self):
        proc = self.run_nanopy_code(
            """
            x = 0
            for i in range(5):
                x = x + i
            print(x)
            """
        )
        self.assert_ok(proc)
        self.assertEqual(proc.stdout.strip(), "10")

    def test_script_mode_and_argv(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            script = td_path / "main.py"
            script.write_text(
                textwrap.dedent(
                    """
                    import sys
                    print(len(sys.argv))
                    print(sys.argv[1])
                    print(sys.argv[2])
                    """
                ).strip()
                + "\n",
                encoding="utf-8",
            )
            proc = self.run_nanopy(str(script), "one", "two", cwd=td_path)
            self.assert_ok(proc)
            self.assertEqual(proc.stdout.strip().splitlines(), ["3", "one", "two"])

    def test_package_dir_execution(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            pkg = td_path / "pkg"
            pkg.mkdir()
            (pkg / "__main__.py").write_text("print('pkg_main')\n", encoding="utf-8")
            proc = self.run_nanopy(str(pkg), cwd=td_path)
            self.assert_ok(proc)
            self.assertEqual(proc.stdout.strip(), "pkg_main")

    def test_module_execution_with_dash_m(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            (td_path / "modx.py").write_text(
                textwrap.dedent(
                    """
                    import sys
                    print(__name__)
                    print(sys.argv[0])
                    print(sys.argv[1])
                    """
                ).strip()
                + "\n",
                encoding="utf-8",
            )
            proc = self.run_nanopy("-m", "modx", "argX", cwd=td_path, pythonpath=[td_path])
            self.assert_ok(proc)
            self.assertEqual(proc.stdout.strip().splitlines(), ["__main__", "modx", "argX"])

    def test_import_and_from_import_alias(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            (td_path / "a.py").write_text("VALUE = 41\n", encoding="utf-8")
            proc = self.run_nanopy_code(
                """
                import a
                from a import VALUE as V
                print(a.VALUE, V)
                """,
                cwd=td_path,
                pythonpath=[td_path],
            )
            self.assert_ok(proc)
            self.assertEqual(proc.stdout.strip(), "41 41")

    def test_dataclass_and_enum_builtin_modules(self):
        proc = self.run_nanopy_code(
            """
            from dataclasses import dataclass, field
            from enum import Enum, auto

            @dataclass
            class User:
                name: str
                age: int = 5
                tags: list = field(default_factory=list)

            class Color(Enum):
                RED = auto()
                BLUE = auto()

            u = User("alice")
            u.tags.append("x")
            v = User("bob")

            print(u.name, u.age, len(u.tags), len(v.tags))
            print(Color.RED.name, Color.RED.value, Color.BLUE.value)
            """
        )
        self.assert_ok(proc)
        self.assertEqual(proc.stdout.strip().splitlines(), ["alice 5 1 0", "RED 1 2"])

    def test_os_and_file_operations(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            proc = self.run_nanopy_code(
                """
                import os
                base = "tmpd"
                os.makedirs(base)
                p = os.path.join(base, "data.txt")
                with open(p, "w") as f:
                    f.write("hello")
                with open(p, "r") as f:
                    txt = f.read()
                print(os.path.exists(p), os.path.basename(p), txt)
                print(os.path.dirname(p))
                os.remove(p)
                print(os.path.exists(p))
                """,
                cwd=td_path,
            )
            self.assert_ok(proc)
            self.assertEqual(proc.stdout.strip().splitlines(), ["True data.txt hello", "tmpd", "False"])

    def test_list_dict_set_operations(self):
        proc = self.run_nanopy_code(
            """
            xs = [3, 1, 2]
            xs.sort()
            xs.append(4)
            xs.extend([5, 6])
            xs.remove(1)
            print(xs.pop(), xs)

            d = {"a": 1}
            print(d.get("a"), d.get("x", 9))
            d.setdefault("x", 7)
            print(len(d.keys()), len(d.values()), len(d.items()))

            s = set([1, 2])
            s.add(3)
            s.update([4, 5])
            s.remove(2)
            print(s.get(3), s.get(99, -1), len(s))
            """
        )
        self.assert_ok(proc)
        self.assertEqual(
            proc.stdout.strip().splitlines(),
            ["6 [2, 3, 4, 5]", "1 9", "2 2 2", "3 -1 4"],
        )

    def test_control_flow_if_while_for_range(self):
        proc = self.run_nanopy_code(
            """
            acc = 0
            i = 0
            while i < 6:
                i = i + 1
                if i % 2 == 0:
                    continue
                if i > 5:
                    break
                acc = acc + i

            total = 0
            for x in range(1, 7, 2):
                total = total + x
            print(acc, total)
            """
        )
        self.assert_ok(proc)
        self.assertEqual(proc.stdout.strip(), "9 9")

    def test_with_and_yield_generator(self):
        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            proc = self.run_nanopy_code(
                """
                def gen(n):
                    i = 0
                    while i < n:
                        yield i
                        i = i + 1

                out = []
                for x in gen(4):
                    out.append(x)

                with open("y.txt", "w") as f:
                    f.write(str(len(out)))
                with open("y.txt", "r") as f:
                    print(out, f.read())
                """,
                cwd=td_path,
            )
            self.assert_ok(proc)
            self.assertEqual(proc.stdout.strip(), "[0, 1, 2, 3] 4")

    def test_gc_cycle_collection_and_flags(self):
        proc = self.run_nanopy_code(
            """
            import gc
            print(gc.isenabled())
            gc.disable()
            print(gc.isenabled())
            gc.enable()
            print(gc.isenabled())

            a = []
            b = []
            a.append(b)
            b.append(a)
            a = None
            b = None
            print(gc.collect() > 0)
            """
        )
        self.assert_ok(proc)
        self.assertEqual(proc.stdout.strip().splitlines(), ["True", "False", "True", "True"])

    def test_try_except_finally_and_raise(self):
        proc = self.run_nanopy_code(
            """
            try:
                raise "boom"
            except Exception as e:
                print(e.message)
            finally:
                print("done")
            """
        )
        self.assert_ok(proc)
        self.assertEqual(proc.stdout.strip().splitlines(), ["boom", "done"])

    def test_comprehensions_lambda_and_fstring(self):
        proc = self.run_nanopy_code(
            """
            vals = [x * x for x in range(5) if x % 2 == 0]
            d = {x: x + 1 for x in range(3)}
            s = {x for x in range(4) if x > 1}
            g = (x + 10 for x in range(3))
            gg = []
            for x in g:
                gg.append(x)
            inc = lambda x: x + 1
            name = "n"
            print(vals, d[1], len(s), gg, inc(4), f"{name}:{inc(1)}")
            """
        )
        self.assert_ok(proc)
        self.assertEqual(proc.stdout.strip(), "[0, 4, 16] 2 2 [10, 11, 12] 5 n:2")

    def test_class_property_classmethod_staticmethod_and_super(self):
        proc = self.run_nanopy_code(
            """
            class A:
                def __init__(self, x):
                    self.x = x

                @classmethod
                def make(cls, x):
                    return cls(x)

                @staticmethod
                def add(a, b):
                    return a + b

                @property
                def twice(self):
                    return self.x * 2

            class B(A):
                def __init__(self, x):
                    super().__init__(x + 1)

            b = B.make(4)
            print(b.x, b.twice, B.add(3, 4))
            """
        )
        self.assert_ok(proc)
        self.assertEqual(proc.stdout.strip(), "5 10 7")

    def test_blocked_unsupported_modules(self):
        for name in ["asyncio", "threading", "multiprocessing", "pickle", "socket"]:
            proc = self.run_nanopy_code(f"import {name}")
            self.assert_fail(proc)
            self.assertIn(f"nanopy does not support module '{name}'", proc.stderr)

    def test_cpython_extension_import_is_unavailable(self):
        proc = self.run_nanopy_code("import math")
        self.assert_fail(proc)
        self.assertIn("unable to locate module 'math'", proc.stderr)

    def test_dynamic_metaclass_is_not_supported(self):
        proc = self.run_nanopy_code(
            """
            class M(type):
                pass

            class X(metaclass=M):
                pass
            """
        )
        self.assert_fail(proc)

    def test_extra_builtin_modules_smoke(self):
        proc = self.run_nanopy_code(
            """
            import argparse
            import pathlib
            import typing
            import copy
            import abc
            import keyword
            from __future__ import annotations

            ns = argparse.Namespace()
            ns.x = 3
            p = pathlib.Path(".")
            arr = [1, [2]]
            dup = copy.deepcopy(arr)
            dup[1].append(3)
            print(hasattr(ns, "x"), p.exists(), typing.List, arr[1], dup[1], keyword.iskeyword("while"), annotations)
            """
        )
        self.assert_ok(proc)
        self.assertEqual(proc.stdout.strip(), "True True List [2] [2, 3] True True")

    def test_fory_compiler_codegen_consistency_with_cpython(self):
        compiler_root = REPO_ROOT / "fory-main" / "compiler"
        demo_file = compiler_root / "examples" / "demo.fdl"
        if not demo_file.exists():
            self.skipTest("fory compiler demo input not found")

        with tempfile.TemporaryDirectory() as td:
            td_path = Path(td)
            out_cpy = td_path / "cpy"
            out_nanopy = td_path / "nanopy"
            out_cpy.mkdir()
            out_nanopy.mkdir()
            env = os.environ.copy()
            env["PYTHONPATH"] = str(compiler_root)

            cpy = subprocess.run(
                [sys.executable, "-m", "fory_compiler", str(demo_file), "--output", str(out_cpy)],
                cwd=str(REPO_ROOT),
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertEqual(cpy.returncode, 0, msg=f"cpython failed\nstdout:\n{cpy.stdout}\nstderr:\n{cpy.stderr}")

            nano = subprocess.run(
                [str(self.nanopy_bin), "-m", "fory_compiler", str(demo_file), "--output", str(out_nanopy)],
                cwd=str(REPO_ROOT),
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertEqual(
                nano.returncode,
                0,
                msg=f"nanopy failed\nstdout:\n{nano.stdout}\nstderr:\n{nano.stderr}",
            )

            cpy_files = sorted(p.relative_to(out_cpy) for p in out_cpy.rglob("*") if p.is_file())
            nano_files = sorted(p.relative_to(out_nanopy) for p in out_nanopy.rglob("*") if p.is_file())
            self.assertEqual(cpy_files, nano_files)

            for rel_path in cpy_files:
                left = (out_cpy / rel_path).read_bytes()
                right = (out_nanopy / rel_path).read_bytes()
                self.assertEqual(left, right, msg=f"content mismatch for {rel_path}")


if __name__ == "__main__":
    unittest.main()
