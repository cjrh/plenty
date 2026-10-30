"""Exercise release auditing and archive contents without compiling Polars."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
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
        return subprocess.run(["python3", "-B", str(RELEASE / "package-dataframe.py"),
                               "--engine", str(self.engine), "--out-dir", str(self.root / directory),
                               "--rust-sysroot", str(self.root / "rust")],
                              env=self.env, capture_output=True, text=True)

    def test_reproducible_archive_metadata_notices_and_permissions(self):
        first = self.package()
        self.assertEqual(first.returncode, 0, first.stderr)
        second = self.package("second")
        self.assertEqual(second.returncode, 0, second.stderr)
        archive = next((self.root / "dist").glob("*.tar.gz"))
        other = next((self.root / "second").glob("*.tar.gz"))
        self.assertEqual(archive.read_bytes(), other.read_bytes())
        self.assertEqual(archive.with_name(archive.name + ".sha256").read_text(),
                         f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")
        name = archive.name.removesuffix(".tar.gz")
        with tarfile.open(archive) as tar:
            metadata = json.load(tar.extractfile(f"{name}/engine.json"))
            self.assertEqual(metadata["target"], f"{os.uname().machine}-unknown-linux-gnu")
            self.assertEqual(metadata["minimum_glibc"], "2.28")
            self.assertEqual(metadata["library_sha256"], hashlib.sha256(self.engine.read_bytes()).hexdigest())
            for path in ["lib/" + LIBRARY, "engine.json", "THIRD-PARTY-NOTICES.md", "licenses/rust/LICENSE.txt"]:
                entry = tar.getmember(f"{name}/{path}")
                self.assertEqual((entry.mode, entry.uid, entry.gid, entry.mtime), (0o644, 0, 0, 1234567890))
            readme = tar.extractfile(f"{name}/README.md").read().decode()
            self.assertNotIn("@VERSION@", readme)
            self.assertIn("same Plenty release", readme)

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


if __name__ == "__main__":
    unittest.main()
