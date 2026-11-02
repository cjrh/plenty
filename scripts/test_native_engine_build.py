"""Native-engine CPU baseline and cross-target artifact selection."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent


class EngineBuildTarget(unittest.TestCase):
    def build(self, host, target=None, name="linalg"):
        spec = importlib.util.spec_from_file_location("engine_builder", ROOT / "scripts/build_native_engine.py")
        builder = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(builder)
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        output = root / "installed"
        builds = root / "builds"
        argv = ["build_native_engine.py", name, "--out", str(output), "--target-dir", str(builds)]
        if target:
            argv += ["--target", target]
        library_name = f"plenty_{name}_v1.dll" if (target or host).endswith("windows-msvc") else f"libplenty_{name}_v1.dylib" if (target or host).endswith("apple-darwin") else f"libplenty_{name}_v1.so"
        calls = []

        def invoke(command, **kwargs):
            calls.append((command, kwargs["env"]))
            selected = command[command.index("--target") + 1]
            library = builds / selected / "release" / library_name
            library.parent.mkdir(parents=True)
            library.write_bytes(b"engine")

        with patch("sys.argv", argv), patch.object(builder.subprocess, "check_output", return_value=f"host: {host}\n"), \
             patch.object(builder.subprocess, "run", invoke), \
             patch.dict(os.environ, {"CARGO_ENCODED_RUSTFLAGS": "-Ctarget-cpu=native", "CFLAGS": "-march=native"}):
            builder.main()
        self.assertEqual((output / library_name).read_bytes(), b"engine")
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

    def test_native_windows_dataframe_uses_msvc_without_elf_or_gcc_flags(self):
        _, env = self.build("x86_64-pc-windows-msvc", name="dataframe")
        self.assertEqual(env["RUSTFLAGS"], "-C target-cpu=x86-64")
        self.assertEqual(env["CFLAGS"], "")
        self.assertEqual(env["CXXFLAGS"], "")
        self.assertEqual(env["CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER"], "link.exe")

    def test_windows_cross_build_and_linalg_are_rejected(self):
        with self.assertRaises(SystemExit):
            self.build("x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc", name="dataframe")
        with self.assertRaises(SystemExit):
            self.build("x86_64-pc-windows-msvc")

    def test_native_macos_dataframe_sets_baseline_and_relocatable_identity(self):
        with patch.dict(os.environ, {"MACOSX_DEPLOYMENT_TARGET": "15.0"}):
            _, env = self.build("aarch64-apple-darwin", name="dataframe")
        self.assertEqual(env["CFLAGS"], "")
        self.assertEqual(env["MACOSX_DEPLOYMENT_TARGET"], "11.0")
        self.assertEqual(env["RUSTFLAGS"], "-C target-cpu=apple-m1 -C link-arg=-Wl,-install_name,@rpath/libplenty_dataframe_v1.dylib")
        self.assertEqual(env["CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER"], "cc")

    def test_macos_cross_build_and_linalg_are_rejected(self):
        with self.assertRaises(SystemExit):
            self.build("x86_64-unknown-linux-gnu", "aarch64-apple-darwin", name="dataframe")
        with self.assertRaises(SystemExit):
            self.build("aarch64-apple-darwin")
