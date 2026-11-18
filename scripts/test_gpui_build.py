"""Exercise the production GPUI link helper without compiling the GUI graph."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("gpui_build", ROOT / "experiments/packages/gpui/build.py")
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)


class NativeLinkInputs(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.manifest = self.root / "Cargo.toml"
        self.archive = self.root / "selected.a"
        self.archive.write_bytes(b"reported archive")
        common = {"manifest_path": str(self.manifest.resolve()),
                  "target": {"name": "plenty_gpui", "crate_types": ["rlib", "staticlib"]}}
        self.artifact = {**common, "reason": "compiler-artifact",
                         "filenames": [str(self.archive), str(self.root / "ignored.rlib")]}
        self.report = {**common, "reason": "compiler-message",
                       "message": {"message": "native-static-libs: -lfirst -lsecond -lfirst",
                                   "rendered": "\x1b[1mnote:\x1b[0m ignored rendered text"}}

    def select(self, messages):
        return BUILD.native_link_inputs("\n".join(map(json.dumps, messages)), self.manifest, "plenty_gpui")

    def test_matching_records_preserve_order_duplicates_and_ignore_rendering(self):
        for cached in [False, True]:
            with self.subTest(cached=cached):
                artifact = {**self.artifact, "fresh": cached}
                other_manifest = {**artifact, "manifest_path": str(self.root / "dependency/Cargo.toml")}
                other_target = {**artifact, "target": {"name": "dependency", "crate_types": ["staticlib"]}}
                archive, args = self.select([other_manifest, other_target, artifact, self.report])
                self.assertEqual(archive, self.archive)
                self.assertEqual(args, ["-lfirst", "-lsecond", "-lfirst"])

    def test_missing_ambiguous_or_nonexistent_inputs_fail_closed(self):
        for messages in [[], [self.artifact], [self.report],
                         [self.artifact, self.artifact, self.report],
                         [self.artifact, self.report, self.report],
                         [{**self.artifact, "filenames": [str(self.archive), str(self.root / "other.a")]}, self.report],
                         [{**self.artifact, "filenames": [str(self.root / "missing.a")]}, self.report]]:
            with self.subTest(messages=messages), self.assertRaises(ValueError):
                self.select(messages)

    def test_unstructured_output_is_not_used_as_a_fallback(self):
        with self.assertRaises(ValueError):
            BUILD.native_link_inputs("note: native-static-libs: -lc", self.manifest, "plenty_gpui")


@unittest.skipUnless(platform.system() == "Linux" and platform.machine() == "x86_64"
                     and all(shutil.which(tool) for tool in ["cargo", "rustc", "cc"]),
                     "requires the qualified Linux x86-64 host and native build tools")
class GpuiBuildMain(unittest.TestCase):
    def test_main_links_reported_fresh_cached_host_target_and_default_archives(self):
        root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix="gpui build ")))
        (root / "examples").mkdir()
        (root / "examples/counter.plenty").write_text("ignored by the compiler stub\n")
        (root / "src").mkdir()
        for name in ["lib.plenty", "native.plentyi"]:
            (root / "src" / name).write_text("")
        (root / "native/src").mkdir(parents=True)
        manifest = root / "native/Cargo.toml"
        manifest.write_text('[package]\nname="plenty-gpui"\nversion="0.0.1"\nedition="2024"\n'
                            '[lib]\ncrate-type=["rlib", "staticlib"]\n[features]\napplication=[]\n[workspace]\n')
        (root / "native/src/lib.rs").write_text('#[unsafe(no_mangle)]\npub extern "C" fn support_value() -> i32 {37}\n')
        # Stub only Plenty emission. Production main performs the actual Cargo
        # invocation, selects its inputs, links with cc and produces a runnable ELF.
        compiler = root / "compiler"
        compiler.write_text(f"#!{sys.executable}\nimport subprocess, sys\n"
                            "subprocess.run(['cc', '-x', 'c', '-c', '-o', sys.argv[sys.argv.index('-o')+1], '-'], "
                            "input='extern int support_value(void); int main(void) { return support_value() == 37 ? 0 : 99; }', "
                            "text=True, check=True)\n")
        compiler.chmod(0o755)
        env = {"CARGO_NET_OFFLINE": "true", "CARGO_TERM_COLOR": "always"}
        subprocess.run(["cargo", "generate-lockfile", "--offline", "--manifest-path", str(manifest)],
                       check=True, capture_output=True, env={**os.environ, **env})
        version = subprocess.check_output(["rustc", "-vV"], text=True)
        host = next(line.removeprefix("host: ") for line in version.splitlines() if line.startswith("host: "))
        stale = root / "native/target/debug/libplenty_gpui.a"
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b"stale default-path archive; must not be linked")
        with patch.object(BUILD, "PACKAGE", root), patch("sys.argv", ["build.py", "--plenty", str(compiler)]), \
             patch.dict(os.environ, env):
            for target in [host, None]:
                if target is None:
                    os.environ.pop("CARGO_BUILD_TARGET", None)
                    stale.unlink()
                else:
                    os.environ["CARGO_BUILD_TARGET"] = target
                for cached in [False, True]:
                    with self.subTest(target=target, cached=cached), contextlib.redirect_stdout(io.StringIO()):
                        BUILD.main()
                        out = root / "target/optimized"
                        log = (out / "native-build.log").read_text()
                        self.assertIn("\x1b[", log)  # forced-color Cargo stderr is still present
                        records = [json.loads(line) for line in log.splitlines() if line.startswith("{")]
                        artifacts = [record for record in records if record["reason"] == "compiler-artifact"
                                     and record["target"]["name"] == "plenty_gpui"]
                        self.assertEqual(len(artifacts), 1)
                        self.assertEqual(artifacts[0]["fresh"], cached)
                        command = shlex.split((out / "link-command.txt").read_text())
                        self.assertIn(command[2], artifacts[0]["filenames"])
                        self.assertTrue(command[2].endswith(".a"))
                        self.assertNotIn("\x1b", (out / "native-link-args.txt").read_text())
                        if target is not None:
                            self.assertNotEqual(Path(command[2]), stale)
                            self.assertEqual(stale.read_bytes(), b"stale default-path archive; must not be linked")
                        subprocess.run([str(out / "plenty-gpui")], check=True, capture_output=True)


if __name__ == "__main__":
    unittest.main()
