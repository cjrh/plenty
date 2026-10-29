"""Local recipes and packaging, without rebuilding Polars/faer for script tests."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parent.parent


def packager():
    spec = importlib.util.spec_from_file_location("package_engine", REPO / "scripts/release/package-engine.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class LocalPackaging(unittest.TestCase):
    def setUp(self):
        self.module = packager()
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix="local build ")))
        self.out = self.root / "installed engines"
        self.out.mkdir()
        self.sysroot = self.root / "rust"
        rust_docs = self.sysroot / "share/doc/rust"
        (rust_docs / "licenses").mkdir(parents=True)
        (rust_docs / "licenses/LICENSE.txt").write_text("Rust license")
        (rust_docs / "COPYRIGHT-library.html").write_text("Rust copyright")
        self.calls = []

    def output(self, *args):
        if args[0] == "readelf":
            return "GLIBC_2.9 GLIBC_2.34 GLIBC_2.3"
        if args[0] == "git":
            if "status" in args:
                return " M plenty-linalg/src/lib.rs"
            return "1234567890" if "log" in args else "a" * 40
        if args[-1] == "--print":
            return "0.6.0"
        raise AssertionError(args)

    def run_command(self, args, **kwargs):
        self.calls.append(args)
        if "--out" in args:
            Path(args[args.index("--out") + 1]).write_text("Third-party licenses")

    def invoke(self, name="linalg", local=True):
        engine = self.out / f"libplenty_{name}_v1.so"
        engine.write_bytes(b"engine bytes")
        argv = ["package-engine.py", name, "--engine", str(engine), "--out-dir", str(self.out),
                "--rust-sysroot", str(self.sysroot)]
        if local:
            argv.append("--local-install")
        with patch("sys.argv", argv), patch.object(self.module, "output", self.output), \
             patch.object(self.module, "check_exports") as exports, \
             patch.object(self.module.subprocess, "run", self.run_command):
            self.module.main()
        exports.assert_called_once_with(engine.resolve(), name)
        return engine

    def test_both_local_layouts_have_matching_hash_and_real_symbol_floor(self):
        for name in ("dataframe", "linalg"):
            engine = self.invoke(name)
            notes = self.out / f"{name}-notices"
            manifest = json.loads((notes / "engine.json").read_text())
            self.assertEqual(manifest["library_sha256"], hashlib.sha256(engine.read_bytes()).hexdigest())
            self.assertEqual(manifest["minimum_glibc"], "2.34")
            self.assertEqual(manifest["build_kind"], "local")
            self.assertTrue(manifest["source_dirty"])
            self.assertEqual(manifest["library"], f"lib/{engine.name}")
            for filename in ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md",
                             "licenses/rust/LICENSE.txt", "licenses/rust/COPYRIGHT-library.html"):
                self.assertTrue((notes / filename).is_file(), filename)
            self.assertFalse((notes / "lib").exists())
            self.assertIn("--local-engine", self.calls[-2])
        self.assertFalse(list(self.out.glob("*.tar.gz")))

    def test_repeated_install_replaces_only_its_generated_notices(self):
        self.invoke()
        notes = self.out / "linalg-notices"
        (notes / "obsolete").write_text("old")
        unrelated = self.out / "my-program"
        unrelated.write_text("keep")
        self.invoke()
        self.assertFalse((notes / "obsolete").exists())
        self.assertEqual(unrelated.read_text(), "keep")
        self.assertFalse(list(self.out.glob(".plenty-*")))

    def test_missing_licenses_preserve_old_notices(self):
        self.invoke()
        before = (self.out / "linalg-notices/engine.json").read_bytes()
        (self.sysroot / "share/doc/rust/licenses/LICENSE.txt").unlink()
        with self.assertRaisesRegex(SystemExit, "no Rust standard-library"):
            self.invoke()
        self.assertEqual((self.out / "linalg-notices/engine.json").read_bytes(), before)

    def test_local_install_requires_library_beside_notices(self):
        engine = self.root / "libplenty_linalg_v1.so"
        engine.write_bytes(b"engine")
        argv = ["package-engine.py", "linalg", "--engine", str(engine),
                "--out-dir", str(self.out), "--local-install"]
        with patch("sys.argv", argv), self.assertRaises(SystemExit) as error:
            self.module.main()
        self.assertEqual(error.exception.code, 2)
        self.assertEqual(list(self.out.iterdir()), [])

    def test_failed_export_check_never_replaces_notices(self):
        self.invoke()
        before = (self.out / "linalg-notices/engine.json").read_bytes()
        with patch.object(self.module, "check_exports", side_effect=SystemExit("missing exports")):
            engine = self.out / "libplenty_linalg_v1.so"
            argv = ["package-engine.py", "linalg", "--engine", str(engine),
                    "--out-dir", str(self.out), "--local-install"]
            with patch("sys.argv", argv), patch.object(self.module.subprocess, "run", self.run_command), \
                 self.assertRaisesRegex(SystemExit, "missing exports"):
                self.module.main()
        self.assertEqual((self.out / "linalg-notices/engine.json").read_bytes(), before)

    def test_release_default_keeps_baseline_audit_and_archive(self):
        self.invoke(local=False)
        self.assertIn("--engine", self.calls[0])
        self.assertNotIn("--local-engine", self.calls[0])
        self.assertTrue(list(self.out.glob("*.tar.gz")))
        self.assertFalse((self.out / "linalg-notices").exists())

    def test_install_refuses_symlink_and_rolls_back_failed_publish(self):
        root = self.root / "staged"
        root.mkdir()
        dest = self.out / "notices"
        dest.symlink_to(root, target_is_directory=True)
        with self.assertRaisesRegex(SystemExit, "not a regular"):
            self.module.install_notices(root, dest)
        dest.unlink()
        dest.mkdir()
        (dest / "old").write_text("preserved")
        rename = Path.rename

        def fail_stage(path, target):
            if path == root:
                raise OSError("publish failed")
            return rename(path, target)

        with patch.object(Path, "rename", fail_stage), self.assertRaises(OSError):
            self.module.install_notices(root, dest)
        self.assertEqual((dest / "old").read_text(), "preserved")


class ElfPolicy(unittest.TestCase):
    def test_local_only_relaxes_symbol_baseline_not_soname(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            readelf = root / "readelf"
            readelf.write_text('''#!/bin/sh
case "$1" in
-h) printf 'Machine: Advanced Micro Devices X86-64\nType: DYN\n';;
-l) :;;
-d) printf ' (SONAME) [%s]\n (NEEDED) [libc.so.6]\n' "$TEST_SONAME";;
-V) echo GLIBC_2.34;;
*) :;;
esac
''')
            readelf.chmod(0o755)
            env = {**os.environ, "PATH": f"{root}:{os.environ['PATH']}",
                   "TEST_SONAME": "libplenty_linalg_v1.so"}
            script = REPO / "scripts/release/check-elf.sh"
            for option, soname, success in [("--engine", env["TEST_SONAME"], False),
                                            ("--local-engine", env["TEST_SONAME"], True),
                                            ("--local-engine", "wrong.so", False)]:
                result = subprocess.run([script, option, "libplenty_linalg_v1.so", "fixture.so"],
                                        env={**env, "TEST_SONAME": soname}, capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, success, result.stderr)


@unittest.skipUnless(shutil.which("just"), "just is not installed")
class Recipes(unittest.TestCase):
    def test_default_build_and_individual_recipes(self):
        with tempfile.TemporaryDirectory(prefix="recipe test ") as directory:
            root = Path(directory)
            log = root / "calls"
            for command in ("cargo", "python3"):
                executable = root / command
                executable.write_text(f'#!/bin/sh\nprintf "%s\\n" "{command} $*" >> "$CALL_LOG"\n')
                executable.chmod(0o755)
            env = {**os.environ, "PATH": f"{root}:{os.environ['PATH']}", "CALL_LOG": str(log)}
            just = shutil.which("just")
            for recipe, expected_count in [([], 5), (["build"], 5), (["compiler"], 1),
                                           (["engine", "linalg"], 2), (["test"], 1), (["docs"], 3)]:
                log.write_text("")
                result = subprocess.run([just, "--justfile", str(REPO / "justfile"), *recipe],
                                        cwd=root, env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                calls = log.read_text().splitlines()
                self.assertEqual(len(calls), expected_count, calls)
                if not recipe or recipe == ["build"]:
                    self.assertIn("--target-dir target", calls[0])
                    self.assertIn("dataframe", calls[1])
                    self.assertIn("--local-install", calls[2])
                    self.assertIn("linalg", calls[3])
                    self.assertIn("--local-install", calls[4])
            (root / "cargo").write_text('#!/bin/sh\nexit 23\n')
            log.write_text("")
            result = subprocess.run([just, "--justfile", str(REPO / "justfile")],
                                    env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(log.read_text(), "")


if __name__ == "__main__":
    unittest.main()
