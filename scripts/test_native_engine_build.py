"""Native-engine CPU baseline and cross-target artifact selection."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent


class EngineBuildTarget(unittest.TestCase):
    def build(self, host, target=None):
        spec = importlib.util.spec_from_file_location("engine_builder", ROOT / "scripts/build_native_engine.py")
        builder = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(builder)
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        output = root / "installed"
        builds = root / "builds"
        argv = ["build_native_engine.py", "linalg", "--out", str(output), "--target-dir", str(builds)]
        if target:
            argv += ["--target", target]
        calls = []

        def invoke(command, **kwargs):
            calls.append((command, kwargs["env"]))
            selected = command[command.index("--target") + 1]
            library = builds / selected / "release/libplenty_linalg_v1.so"
            library.parent.mkdir(parents=True)
            library.write_bytes(b"engine")

        with patch("sys.argv", argv), patch.object(builder.subprocess, "check_output", return_value=f"host: {host}\n"), \
             patch.object(builder.subprocess, "run", invoke), \
             patch.dict(os.environ, {"CARGO_ENCODED_RUSTFLAGS": "-Ctarget-cpu=native", "CFLAGS": "-march=native"}):
            builder.main()
        self.assertEqual((output / "libplenty_linalg_v1.so").read_bytes(), b"engine")
        return calls[0]

    def test_native_arm_uses_generic_armv8_and_matching_artifact(self):
        command, env = self.build("aarch64-unknown-linux-gnu")
        self.assertEqual(command[command.index("--target") + 1], "aarch64-unknown-linux-gnu")
        self.assertIn("target-cpu=generic", env["RUSTFLAGS"])
        self.assertEqual(env["CFLAGS"], "-march=armv8-a -mtune=generic")
        self.assertNotIn("CARGO_ENCODED_RUSTFLAGS", env)
        self.assertEqual(env["CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER"], "cc")

    def test_cross_arm_uses_explicit_linker_and_x86_keeps_original_baseline(self):
        _, env = self.build("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")
        self.assertEqual(env["CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER"], "aarch64-linux-gnu-gcc")
        _, env = self.build("x86_64-unknown-linux-gnu")
        self.assertIn("target-cpu=x86-64", env["RUSTFLAGS"])
        self.assertEqual(env["CFLAGS"], "-march=x86-64 -mtune=generic")
