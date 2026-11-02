"""Exercise release auditing and the bundled engine layout without compiling Polars."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parent.parent
RELEASE = REPO / "scripts/release"
LIBRARY = "libplenty_dataframe_v1.so"


@unittest.skipUnless(Path("/usr/bin/cc").exists() and shutil.which("readelf") and shutil.which("nm"), "requires system C compiler and binutils")
class EnginePackage(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix="engine package ")))
        self.engine = self.root / LIBRARY
        self.rust = self.root / "rust/share/doc/rust"
        (self.rust / "licenses").mkdir(parents=True)
        (self.rust / "COPYRIGHT-library.html").write_text("Rust notices")
        (self.rust / "licenses/LICENSE.txt").write_text("Rust license")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        metadata = {"packages": [{"id": "engine", "name": "plenty-dataframe-engine", "source": None}],
                    "resolve": {"nodes": [{"id": "engine", "deps": []}]}}
        cargo = self.bin / "cargo"
        cargo.write_text("#!/bin/sh\ncat <<'JSON'\n" + json.dumps(metadata) + "\nJSON\n")
        cargo.chmod(0o755)
        self.env = {**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}", "SOURCE_DATE_EPOCH": "1234567890"}
        self.exports = sorted(set(re.findall(r'= "(pldf_v1_\w+)"', (REPO / "stdlib/std/dataframe.plentyi").read_text())))
        self.compile(self.exports)

    def compile(self, exports, soname=LIBRARY, rpath=None):
        source = self.root / "engine.c"
        # Keep one libc reference so ELF version auditing has a real baseline.
        source.write_text('#include <stdio.h>\nvoid fixture(void) { puts("fixture"); }\n' +
                          "\n".join(f"void {name}(void) {{}}" for name in exports))
        command = ["/usr/bin/cc", "-shared", "-fPIC", str(source), f"-Wl,-soname,{soname}", "-o", str(self.engine)]
        if rpath:
            command.append(f"-Wl,-rpath,{rpath}")
        subprocess.run(command, check=True, capture_output=True)

    def package(self, directory="dist"):
        return subprocess.run(["python3", "-B", str(RELEASE / "package-engine.py"), "dataframe",
                               "--engine", str(self.engine), "--out-dir", str(self.root / directory),
                               "--rust-sysroot", str(self.root / "rust")],
                              env=self.env, capture_output=True, text=True)

    def test_installs_library_and_release_notices(self):
        first = self.package()
        self.assertEqual(first.returncode, 0, first.stderr)
        second = self.package("second")
        self.assertEqual(second.returncode, 0, second.stderr)
        out = self.root / "dist"
        self.assertEqual((out / LIBRARY).read_bytes(), self.engine.read_bytes())
        self.assertEqual((out / LIBRARY).stat().st_mode & 0o777, 0o644)
        notices = out / "dataframe-notices"
        metadata = json.loads((notices / "engine.json").read_text())
        self.assertEqual(metadata["target"], f"{os.uname().machine}-unknown-linux-gnu")
        self.assertEqual(metadata["minimum_glibc"], "2.28")
        self.assertNotIn("build_kind", metadata)
        self.assertEqual(metadata["library_sha256"], hashlib.sha256(self.engine.read_bytes()).hexdigest())
        for path in ["engine.json", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md",
                     "licenses/rust/LICENSE.txt", "licenses/rust/COPYRIGHT-library.html"]:
            self.assertEqual((notices / path).read_bytes(),
                             (self.root / "second/dataframe-notices" / path).read_bytes(), path)
        self.assertEqual(sorted(p.name for p in out.iterdir()), sorted([LIBRARY, "dataframe-notices"]))

    def test_missing_export_is_rejected_before_packaging(self):
        self.compile(self.exports[1:])
        result = self.package()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(self.exports[0], result.stderr)
        self.assertFalse((self.root / "dist").exists())

    def test_missing_runtime_license_texts_are_not_silently_omitted(self):
        (self.rust / "licenses/LICENSE.txt").unlink()
        result = self.package()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no Rust standard-library license texts", result.stderr)

    def test_incorrect_soname_and_embedded_search_path_are_rejected(self):
        self.compile(self.exports, soname="wrong.so")
        result = self.package()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SONAME", result.stderr)
        self.compile(self.exports, rpath="/build/private")
        result = self.package()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("RPATH/RUNPATH", result.stderr)


class WindowsEnginePackage(unittest.TestCase):
    def test_installs_audited_dll_with_windows_metadata_and_notices(self):
        import importlib.util
        import test_release_scripts as fixtures
        from unittest.mock import patch
        spec = importlib.util.spec_from_file_location("windows_packager", RELEASE / "package-engine.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory(prefix="Windows engine ") as temporary:
            root = Path(temporary)
            engine = root / "plenty_dataframe_v1.dll"
            exports = re.findall(r'= "(pldf_v1_\w+)"', (REPO / "stdlib/std/dataframe.plentyi").read_text())
            engine.write_bytes(fixtures.CheckNative.pe(dll=True, exports=exports))
            rust = root / "rust/share/doc/rust"
            (rust / "licenses").mkdir(parents=True)
            (rust / "licenses/LICENSE.txt").write_text("Rust license")
            (rust / "COPYRIGHT-library.html").write_text("Rust notices")
            destination = root / "out"
            argv = ["package-engine.py", "dataframe", "--target", "x86_64-pc-windows-msvc",
                    "--engine", str(engine), "--out-dir", str(destination),
                    "--rust-sysroot", str(root / "rust")]
            def command(args, **kwargs):
                self.assertIn("third_party_notices.py", str(args[2]))
                Path(args[args.index("--out") + 1]).write_text("Dependency notices")
            def output(*args):
                return "0.8.3" if args[-1] == "--print" else "a" * 40
            with patch("sys.argv", argv), patch.object(module.subprocess, "run", command), patch.object(module, "output", output):
                module.main()
                manifest = json.loads((destination / "dataframe-notices/engine.json").read_text())
                self.assertEqual(manifest["target"], "x86_64-pc-windows-msvc")
                self.assertEqual(manifest["minimum_windows"], "10")
                self.assertNotIn("minimum_glibc", manifest)
                self.assertEqual((destination / engine.name).read_bytes(), engine.read_bytes())
                self.assertEqual(manifest["library_sha256"], hashlib.sha256(engine.read_bytes()).hexdigest())
                engine.write_bytes(fixtures.CheckNative.pe(dll=True, exports=exports[1:]))
                with self.assertRaisesRegex(SystemExit, "missing exports"):
                    module.main()


if __name__ == "__main__":
    unittest.main()
