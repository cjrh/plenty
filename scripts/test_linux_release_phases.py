"""Exercise Linux release preparation and packaging without a foreign libc."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


def job(workflow, name):
    return re.search(rf"^  {name}:\n(.*?)(?=^  [a-z][a-z-]*:|\Z)",
                     workflow, re.M | re.S).group(1)


class LinuxReleasePhases(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.release = self.root / "scripts/release"
        self.release.mkdir(parents=True)
        for name in ("build.sh", "target.sh", "toolchain.env"):
            shutil.copy2(ROOT / "scripts/release" / name, self.release / name)
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.log = self.root / "calls"
        self.stub("ldd", 'echo "ldd (GNU libc) ${FAKE_GLIBC:-2.28}"\n')
        self.stub("rustc", '''case "$1" in
    -vV) echo "host: ${FAKE_HOST:-aarch64-unknown-linux-gnu}" ;;
    --print) echo /stub/sysroot ;;
    *) echo 'rustc fixture' ;;
esac
''')
        self.stub("cc", 'if [ "$1" = -print-prog-name=ld ]; then echo "$FAKE_BIN/ld"; else echo "cc fixture"; fi\n')
        self.stub("ld", 'echo "GNU ld fixture"\n')
        self.stub("rustup", 'echo "rustup $*" >> "$CALL_LOG"\n')
        self.stub("cargo", '''echo "cargo $*|$RUSTUP_TOOLCHAIN|$RUSTFLAGS|$PLENTY_RUNTIME_RUSTFLAGS|$CFLAGS" >> "$CALL_LOG"
if [ "${FAIL_STAGE:-}" = "$1" ]; then exit 9; fi
''')
        self.stub("python3", '''echo "engine $*" >> "$CALL_LOG"
if [ "${FAIL_STAGE:-}" = engine ]; then exit 9; fi
''')
        for name in ("check-elf.sh", "package.sh"):
            path = self.release / name
            path.write_text(f'#!/usr/bin/env bash\necho "{name} $*" >> "$CALL_LOG"\n'
                            f'if [ "${{FAIL_STAGE:-}}" = "{name}" ]; then exit 9; fi\n')
            path.chmod(0o755)
        self.env = {**os.environ, "PATH": str(self.tools) + os.pathsep + os.environ["PATH"],
                    "FAKE_BIN": str(self.tools), "CALL_LOG": str(self.log),
                    "PLENTY_RELEASE_TARGET": "aarch64-unknown-linux-gnu",
                    "CARGO_TARGET_DIR": str(self.root / "builds"), "CARGO_HOME": str(self.root / "cargo")}

    def stub(self, name, body):
        path = self.tools / name
        path.write_text("#!/usr/bin/env bash\nset -eu\n" + body)
        path.chmod(0o755)

    def run_build(self, *args):
        return subprocess.run(["bash", str(self.release / "build.sh"),
                               "--out-dir", str(self.root / "dist"), *args],
                              env=self.env, cwd=self.root, capture_output=True, text=True)

    def calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []

    def test_prepare_builds_audits_and_runs_full_suite_without_engines(self):
        result = self.run_build("--phase", "prepare", "--test")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 4)
        self.assertIn("cargo build --locked --release --bins", calls[1])
        self.assertTrue(calls[2].startswith("check-elf.sh "))
        self.assertIn("cargo test --locked --workspace --all-features --profile ci", calls[3])
        self.assertIn("|-C target-cpu=generic|-C target-cpu=generic|-march=armv8-a -mtune=generic", calls[1])

    def test_engine_phase_validates_only_selected_engine_without_compiler(self):
        for name in ("dataframe", "linalg"):
            with self.subTest(engine=name):
                self.log.unlink(missing_ok=True)
                result = self.run_build("--phase", "engine", "--engine", name,
                                        "--test", "--engine-workspace-targets")
                self.assertEqual(result.returncode, 0, result.stderr)
                calls = self.calls()
                self.assertEqual(len(calls), 2)
                self.assertIn(f"scripts/build_native_engine.py {name}", calls[1])
                self.assertIn(f"--target-dir {self.root}/plenty-{name}/target", calls[1])
                self.assertIn("--validate", calls[1])

    def test_package_uses_caller_relative_engine_directory_without_compiling(self):
        (self.root / "engines").mkdir()
        result = self.run_build("--phase", "package", "--engine-dir", "engines")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 1)
        self.assertTrue(calls[0].startswith("package.sh "))
        for name in ("dataframe", "linalg"):
            self.assertIn(f"--engine {name}={self.root}/engines/libplenty_{name}_v1.so", calls[0])

    def test_default_retains_every_stage_and_failure_propagation(self):
        result = self.run_build("--test")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 7)
        self.assertIn("dataframe", calls[4])
        self.assertIn("linalg", calls[5])
        self.assertTrue(calls[6].startswith("package.sh "))
        for stage in ("build", "check-elf.sh", "test", "engine"):
            with self.subTest(failure=stage):
                self.log.unlink()
                self.env["FAIL_STAGE"] = stage
                result = self.run_build("--test")
                self.assertEqual(result.returncode, 9, result.stderr)
                self.assertFalse(any(c.startswith("package.sh ") for c in self.calls()))

    def test_invalid_combinations_and_wrong_baselines_fail_before_compilation(self):
        for args in (("--phase", "missing"), ("--phase", "engine"),
                     ("--phase", "engine", "--engine", "dataframe linalg"),
                     ("--phase", "engine", "--engine", "unknown"),
                     ("--phase", "prepare", "--engine", "dataframe"),
                     ("--phase", "package"),
                     ("--phase", "package", "--engine-dir", ".", "--test")):
            with self.subTest(args=args):
                self.assertEqual(self.run_build(*args).returncode, 2)
                self.assertEqual(self.calls(), [])
        for key, value in (("FAKE_GLIBC", "2.39"), ("FAKE_HOST", "x86_64-unknown-linux-gnu")):
            self.env[key] = value
            self.assertNotEqual(self.run_build("--phase", "prepare").returncode, 0)
            self.assertEqual(self.calls(), [])
            del self.env[key]


class LinuxReleaseWorkflow(unittest.TestCase):
    def test_preparations_are_independent_and_packaging_is_a_hard_gate(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        for name in ("build", "engine"):
            body = job(workflow, name)
            self.assertIn("needs: plan", body)
            self.assertNotIn("needs: [", body)
            self.assertIn("image: ${{ matrix.image }}", body)
            self.assertIn("options: --user 1001:1001", body)
            self.assertIn("--test --phase", body)
        self.assertIn("--test --phase prepare", job(workflow, "build"))
        engine = job(workflow, "engine")
        self.assertIn("plenty-${{ matrix.engine }} -> target", engine)
        self.assertIn('--test --phase engine --engine "$ENGINE"', engine)
        package = job(workflow, "package")
        self.assertIn("needs: [plan, build, engine]", package)
        self.assertIn("--phase package --engine-dir artifacts/libraries", package)
        self.assertIn("release-linux-compiler-${{ matrix.target }}", package)
        self.assertIn("release-linux-engine-${{ matrix.target }}-*", package)
        self.assertIn("needs: [plan, package]", job(workflow, "smoke"))
        self.assertIn("build, engine, package, smoke", job(workflow, "publish"))
        self.assertIn("if: github.event_name == 'push'", job(workflow, "publish"))
        self.assertIn("needs: [plan, test, build, engine, package, smoke, native-build, native-engine, native-package, native-smoke, publish]", job(workflow, "timings"))
        self.assertIn("if: always()", job(workflow, "timings"))
