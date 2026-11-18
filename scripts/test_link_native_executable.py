"""Qualify Cargo record selection and the actual final link without a backend."""
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import link_native_executable
from link_native_executable import native_link_inputs


class NativeLinkInputs(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.manifest = self.root / "Cargo.toml"
        self.archive = self.root / "selected.a"
        self.archive.write_bytes(b"reported archive")
        common = {"manifest_path": str(self.manifest.resolve()),
                  "target": {"name": "support", "crate_types": ["rlib", "staticlib"]}}
        self.artifact = {**common, "reason": "compiler-artifact",
                         "filenames": [str(self.archive), str(self.root / "ignored.rlib")]}
        self.report = {**common, "reason": "compiler-message",
                       "message": {"message": "native-static-libs: -lfirst -lsecond -lfirst",
                                   "rendered": "\x1b[1mnote:\x1b[0m ignored rendered text"}}

    def select(self, messages):
        return native_link_inputs("\n".join(map(json.dumps, messages)), self.manifest, "support")

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
            native_link_inputs("note: native-static-libs: -lc", self.manifest, "support")


@unittest.skipUnless(platform.system() == "Linux" and platform.machine() == "x86_64"
                     and all(shutil.which(tool) for tool in ["cargo", "rustc", "cc"]),
                     "requires the qualified Linux x86-64 host and native build tools")
class CargoLink(unittest.TestCase):
    def test_fresh_and_cached_host_target_and_default_archive(self):
        root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix="support build ")))
        manifest = root / "Cargo.toml"
        manifest.write_text('[package]\nname="support"\nversion="0.0.1"\nedition="2024"\n'
                            '[lib]\npath="lib.rs"\ncrate-type=["rlib", "staticlib"]\n'
                            '[features]\napplication=[]\n[workspace]\n')
        (root / "lib.rs").write_text('#[cfg(feature="application")]\n#[unsafe(no_mangle)]\n'
                                     'pub extern "C" fn support_value() -> i32 {37}\n')
        obj = root / "app.o"
        subprocess.run(["cc", "-x", "c", "-c", "-o", obj, "-"], check=True, text=True,
                       input="extern int support_value(void); int main(void) {return support_value() != 37;}")
        subprocess.run(["cargo", "generate-lockfile", "--offline", "--manifest-path", manifest], check=True)
        host = next(line.removeprefix("host: ") for line in subprocess.check_output(
            ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
        target_dir = root / "alternate output"
        stale = target_dir / "debug/libsupport.a"
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b"stale archive")
        executable = root / "app"
        argv = ["link_native_executable.py", "--manifest-path", str(manifest), "--lib-name", "support",
                "--features", "application", "--target-dir", str(target_dir), "-o", str(executable), str(obj)]
        self.enterContext(patch("sys.argv", argv))
        self.enterContext(patch.dict(os.environ, CARGO_NET_OFFLINE="true", CARGO_TERM_COLOR="always",
                                     CARGO_TERM_QUIET="false"))
        run = subprocess.run
        for target in [host, None]:
            if target is None:
                os.environ.pop("CARGO_BUILD_TARGET", None)
                stale.unlink()
            else:
                os.environ["CARGO_BUILD_TARGET"] = target
            for fresh in [False, True]:
                cargo_stdout = []

                def record(command, **kwargs):
                    result = run(command, **kwargs)
                    if command[0] == "cargo":
                        cargo_stdout.append(result.stdout)
                    return result

                with self.subTest(target=target, fresh=fresh), patch.object(subprocess, "run", side_effect=record) as calls:
                    link_native_executable.main()
                    records = [json.loads(line) for line in cargo_stdout[0].splitlines()]
                    artifact, = [r for r in records if r["reason"] == "compiler-artifact"
                                 and r["manifest_path"] == str(manifest) and r["target"]["name"] == "support"]
                    self.assertEqual(artifact["fresh"], fresh)
                    archive, = [Path(path) for path in artifact["filenames"] if path.endswith(".a")]
                    report, = [r["message"]["message"] for r in records if r["reason"] == "compiler-message"
                               and r["manifest_path"] == str(manifest) and r["target"]["name"] == "support"
                               and r["message"]["message"].startswith("native-static-libs: ")]
                    self.assertEqual(calls.call_args.args[0], ["cc", obj, archive, "-Wl,--gc-sections",
                                     *shlex.split(report.removeprefix("native-static-libs: ")), "-o", executable])
                    if target:
                        self.assertNotEqual(archive, stale)
                        self.assertEqual(stale.read_bytes(), b"stale archive")
                    run([executable], check=True)
