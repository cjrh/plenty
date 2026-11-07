"""Exercise native build phase boundaries without requiring a foreign OS/toolchain."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class NativeReleasePhases(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.release = self.root / "scripts/release"
        self.release.mkdir(parents=True)
        for name in ("build-native.sh", "target.sh", "toolchain.env"):
            shutil.copy2(ROOT / "scripts/release" / name, self.release / name)
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.log = self.root / "calls"
        self.stub("rustc", '''case "$1" in
    -vV) echo "host: $PLENTY_RELEASE_TARGET" ;;
    --print) echo /stub/sysroot ;;
    *) echo 'rustc fixture' ;;
esac
''')
        self.stub("cargo", '''echo "cargo $*|$RUSTUP_TOOLCHAIN|$RUSTFLAGS|$PLENTY_RUNTIME_RUSTFLAGS|${MACOSX_DEPLOYMENT_TARGET:-}" >> "$CALL_LOG"
if [ "${FAIL_CARGO:-}" = "$1" ]; then exit 9; fi
''')
        self.stub("engine-python", 'echo "engine $*" >> "$CALL_LOG"\n')
        self.stub("cc", "echo 'fixture cc'\n")
        package = self.release / "package.sh"
        package.write_text('#!/usr/bin/env bash\necho "package $*" >> "$CALL_LOG"\n')
        package.chmod(0o755)
        self.env = {**os.environ, "PATH": str(self.tools) + os.pathsep + os.environ["PATH"],
                    "CALL_LOG": str(self.log), "PYTHON": "engine-python",
                    "PLENTY_RELEASE_TARGET": "aarch64-apple-darwin",
                    "CARGO_TARGET_DIR": str(self.root / "builds")}

    def stub(self, name, body):
        path = self.tools / name
        path.write_text('#!/usr/bin/env bash\nset -eu\n' + body)
        path.chmod(0o755)

    def run_build(self, *args):
        return subprocess.run(["bash", str(self.release / "build-native.sh"),
                               "--out-dir", str(self.root / "dist"), *args],
                              env=self.env, cwd=self.root, capture_output=True, text=True)

    def calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def test_prepare_builds_and_tests_without_building_engine_or_packaging(self):
        result = self.run_build("--phase", "prepare", "--test")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 3)
        self.assertIn("cargo build --locked --release --bins", calls[0])
        self.assertIn("platform-tests/Cargo.toml", calls[1])
        self.assertIn("--workspace --lib --no-fail-fast -- --test-threads=1", calls[2])
        self.assertIn("|-C target-cpu=apple-m1|-C target-cpu=apple-m1|11.0", calls[0])

    def test_default_still_builds_validates_engine_and_packages(self):
        result = self.run_build("--test")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 5)
        self.assertIn("scripts/build_native_engine.py dataframe", calls[3])
        self.assertIn("--validate", calls[3])
        self.assertTrue(calls[4].startswith("package "))
        self.assertIn("--engine dataframe=" + str(self.root / "builds/release/libplenty_dataframe_v1.dylib"), calls[4])

    def test_package_consumes_relative_engine_path_without_compiling(self):
        (self.root / "engines").mkdir()
        self.env["PLENTY_RELEASE_TARGET"] = "x86_64-pc-windows-msvc"
        result = self.run_build("--phase", "package", "--engine-dir", "engines")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 1)
        self.assertIn("--engine dataframe=" + str(self.root / "engines/plenty_dataframe_v1.dll"), calls[0])
        self.assertIn("--bin-dir " + str(self.root / "builds/release"), calls[0])

    def test_failed_compilation_or_tests_prevent_packaging(self):
        for command in ("build", "test"):
            with self.subTest(command=command):
                self.log.unlink(missing_ok=True)
                self.env["FAIL_CARGO"] = command
                result = self.run_build("--test")
                self.assertEqual(result.returncode, 9)
                self.assertFalse(any(line.startswith(("package ", "engine ")) for line in self.calls()))

    def test_invalid_phase_combinations_fail_before_work(self):
        for args in (("--phase", "missing"), ("--phase", "package"),
                     ("--phase", "package", "--engine-dir", ".", "--test"),
                     ("--phase", "prepare", "--engine-dir", ".")):
            with self.subTest(args=args):
                self.assertEqual(self.run_build(*args).returncode, 2)
                self.assertEqual(self.calls(), [])
