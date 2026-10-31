"""Tests for scripts/release: version checks, publication states, and the
consistency of the documented prerequisites with what the smoke test installs.

The archive build and smoke test themselves run through
scripts/release/local-check.sh, which needs Docker; see
book/src/development/releasing.md.
"""

import os
import json
import hashlib
import importlib.util
import re
import shutil
import subprocess
import tempfile
import tomllib
import tarfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
RELEASE = REPO / "scripts" / "release"
INSTALL_PAGE = REPO / "book" / "src" / "design" / "toolchain" / "01-building" / "installing.md"

FAKE_GH = """#!/usr/bin/env bash
echo "gh $*" >> "$FAKE_GH_LOG"
case "$1 $2" in
  "release view")
    case "$FAKE_GH_STATE" in
      missing) echo "release not found" >&2; exit 1 ;;
      broken) echo "HTTP 502" >&2; exit 1 ;;
      draft) echo true ;;
      published) echo false ;;
    esac ;;
  "release download") cp "$FAKE_GH_PUBLISHED"/*.sha256 "${@: -1}/" ;;
esac
"""


def run(args, **kwargs):
    return subprocess.run(args, capture_output=True, text=True, **kwargs)


class ThirdPartyNotices(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location(
            "third_party_notices", RELEASE / "third_party_notices.py"
        )
        self.notices = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.notices)
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.package = {
            "name": "iced-x86", "version": "1.21.0", "license": "MIT",
            "manifest_path": str(self.root / "Cargo.toml"),
        }

    def test_omitted_license_uses_complete_pinned_upstream_text(self):
        output = self.notices.render([self.package])
        license_text = (RELEASE / "licenses" / "iced-x86-1.21.0-LICENSE.txt").read_text()
        self.assertIn(license_text.rstrip(), output)
        self.assertIn("Copyright (C) 2018-present iced project and contributors", output)
        self.assertIn("c50f29b7bc305696895c075f3fc7719751426b12/LICENSE.txt", output)

    def test_git_dependency_records_the_resolved_fork_revision(self):
        source = "git+https://example.test/fork?branch=workspace#0123456789abcdef"
        self.package["source"] = source
        self.assertIn(f"Resolved Git source: {source}", self.notices.render([self.package]))

    def test_unreviewed_package_version_or_license_still_fails(self):
        for change in [{"name": "other"}, {"version": "1.22.0"}, {"license": "Apache-2.0"}]:
            with self.subTest(change=change):
                with self.assertRaisesRegex(SystemExit, "no license file in shipped packages"):
                    self.notices.render([{**self.package, **change}])

    def test_packaged_license_takes_precedence(self):
        (self.root / "LICENSE").write_text("Packaged license text")
        output = self.notices.render([self.package])
        self.assertIn("Packaged license text", output)
        self.assertNotIn("License text source:", output)

    def test_missing_bundled_license_fails(self):
        self.notices.LICENSE_DIR = self.root / "missing"
        with self.assertRaises(FileNotFoundError):
            self.notices.render([self.package])

    def test_compiler_notices_include_embedded_std_sources(self):
        output = self.notices.render([self.package])
        for _, license_id, _, source, name in self.notices.EMBEDDED:
            text = (RELEASE / "licenses" / name).read_text().rstrip()
            self.assertIn(text, output)
            self.assertIn(f"License: {license_id}", output)
            self.assertIn(f"Source: {source}", output)
        self.assertIn("Copyright (c) 2014, John MacFarlane", output)
        self.assertIn("UNICODE LICENSE V3", output)
        self.assertIn("Copyright © WHATWG", output)
        self.assertIn("## similar 3.2.1 (std.diff algorithms)", output)
        self.assertIn("Apache License\n                        Version 2.0", output)

    def test_compiler_notices_include_vendored_unicode_normalization(self):
        output = self.notices.render([self.package])
        vendor = REPO / "plenty-runtime/src/unicode/vendor"
        self.assertIn("## unicode-normalization 0.1.25", output)
        self.assertIn("## Unicode Character Database 17.0.0", output)
        for name in ("COPYRIGHT", "LICENSE-APACHE", "LICENSE-MIT"):
            self.assertIn((vendor / name).read_text().rstrip(), output)
        self.assertIn("UNICODE_VERSION: (u8, u8, u8) = (17, 0, 0)", (vendor / "tables.rs").read_text())
        self.assertNotIn("unicode-normalization", self.notices.render([self.package], "dataframe"))

    def test_every_repository_vendor_directory_has_a_notice(self):
        # Cargo.lock stands in for `cargo metadata`, which this test avoids so
        # it runs without a Rust toolchain; the release build checks the
        # shipped packages themselves.
        lock = tomllib.loads((REPO / "Cargo.lock").read_text())
        packages = [p for p in lock["package"] if "source" in p]
        self.assertEqual(self.notices.uncovered_vendor_dirs(REPO, packages), [])

    def test_uncovered_vendor_directory_fails(self):
        crate = {"name": "covered", "version": "1.0.0"}
        (self.root / "a/vendor/covered-1.0.0").mkdir(parents=True)
        self.assertEqual(self.notices.uncovered_vendor_dirs(self.root, [crate]), [])
        (self.root / "a/vendor/covered-2.0.0").mkdir()
        (self.root / "b/vendor").mkdir(parents=True)
        (self.root / "b/vendor/tables.rs").write_text("")
        (self.root / "plenty-runtime/native/zlib-1.3").mkdir(parents=True)
        (self.root / "target/vendor/build-output").mkdir(parents=True)
        (self.root / "plenty-linalg/vendor/faer").mkdir(parents=True)
        self.assertEqual(
            self.notices.uncovered_vendor_dirs(self.root, [crate]),
            ["a/vendor/covered-2.0.0", "b/vendor", "plenty-runtime/native/zlib-1.3"],
        )
        self.notices.VENDORED_SOURCES = (("", "", "", "", "b/vendor", ()),)
        self.notices.NATIVE_DIRS = ("plenty-runtime/native/zlib-1.3",)
        self.assertEqual(self.notices.uncovered_vendor_dirs(self.root, [crate]),
                         ["a/vendor/covered-2.0.0"])

    def test_missing_embedded_license_fails(self):
        self.notices.LICENSE_DIR = self.root
        (self.root / "iced-x86-1.21.0-LICENSE.txt").write_text("iced")
        with self.assertRaises(FileNotFoundError):
            self.notices.render([self.package])

    def test_direct_vendor_inputs_keep_notices_when_cargo_filters_an_architecture_edge(self):
        vendor = self.root / "plenty-runtime/vendor/cpufeatures-0.3.1"
        vendor.mkdir(parents=True)
        (vendor / "Cargo.toml").write_text('[package]\nname = "cpufeatures"\nversion = "0.3.1"\nlicense = "MIT OR Apache-2.0"\n')
        (vendor / "LICENSE-MIT").write_text("Vendored CPU feature license")
        packages = self.notices.embedded_vendor_packages(self.root, [])
        self.assertEqual([(p["name"], p["version"]) for p in packages], [("cpufeatures", "0.3.1")])
        self.assertIn("Vendored CPU feature license", self.notices.render(packages))

    def test_compiler_bundles_brotli_license_but_dataframe_does_not(self):
        license_text = (REPO / "plenty-runtime/native/brotli-1.2.1/LICENSE").read_text().rstrip()
        compiler = self.notices.render([self.package])
        self.assertIn("## Brotli 1.2.1", compiler)
        self.assertIn(license_text, compiler)
        dataframe = self.notices.render([self.package], "dataframe")
        self.assertNotIn("## Brotli", dataframe)

    def test_dataframe_notices_have_pinned_parquet_licenses_without_sqlite(self):
        nested = self.root / "src/thrift/varint/LICENSE"
        nested.parent.mkdir(parents=True)
        nested.write_text("Nested implementation license")
        package = {**self.package, "name": "polars-parquet-format", "version": "0.1.0", "license": "MIT AND Apache-2.0"}
        output = self.notices.render([package], "dataframe")
        self.assertIn("Copyright (c) 2022 Jorge C Leitao", output)
        self.assertIn("Apache License", output)
        self.assertIn("src/thrift/varint/LICENSE", output)
        self.assertIn("Nested implementation license", output)
        self.assertNotIn("## SQLite", output)
        self.assertNotIn("commonmark.js", output)
        with self.assertRaisesRegex(SystemExit, "no license file"):
            self.notices.render([{**package, "version": "0.2.0"}], "dataframe")


class InstallMold(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.script = self.root / "install-mold.sh"
        shutil.copy(RELEASE / "install-mold.sh", self.script)
        shutil.copy(RELEASE / "target.sh", self.root / "target.sh")
        package = self.root / "mold-test-x86_64-linux"
        (package / "bin").mkdir(parents=True)
        mold = package / "bin" / "mold"
        mold.write_text('#!/bin/sh\necho "mold test"\n')
        mold.chmod(0o755)
        self.archive = self.root / "mold.tar.gz"
        subprocess.run(["tar", "-czf", self.archive, "-C", self.root, package.name], check=True)
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        (self.root / "toolchain.env").write_text(f"MOLD_VERSION=test\nMOLD_SHA256={digest}\n")
        commands = self.root / "commands"
        commands.mkdir()
        curl = commands / "curl"
        curl.write_text('#!/bin/sh\nwhile [ "$1" != -o ]; do shift; done\ncp "$MOLD_TEST_ARCHIVE" "$2"\n')
        curl.chmod(0o755)
        self.env = {**os.environ, "PATH": f"{commands}:{os.environ['PATH']}",
                    "MOLD_TEST_ARCHIVE": str(self.archive), "PLENTY_RELEASE_TARGET": "x86_64-unknown-linux-gnu"}
        self.destination = self.root / "installed linker"

    def test_verified_archive_installs_without_root(self):
        result = run(["bash", self.script, self.destination], env=self.env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("mold test", result.stdout)
        self.assertTrue((self.destination / "bin" / "mold").is_file())

    def test_checksum_failure_never_extracts_or_runs(self):
        self.archive.write_bytes(self.archive.read_bytes() + b"corrupt")
        result = run(["bash", self.script, self.destination], env=self.env)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.destination.exists())
        self.assertNotIn("mold test", result.stdout)

    def test_missing_custom_linker_fails_early(self):
        result = run([REPO / "scripts" / "ci_check_mold.sh"],
                     env={**os.environ, "COMPILER_PATH": str(self.root / "missing")})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("is missing", result.stdout)


class CheckVersion(unittest.TestCase):
    def script_with_version(self, version):
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        (root / "scripts" / "release").mkdir(parents=True)
        shutil.copy(RELEASE / "check-version.sh", root / "scripts" / "release")
        (root / "Cargo.toml").write_text(
            f'[package]\nname = "plenty"\nversion = "{version}"\n\n'
            '[dependencies]\nversion = "9.9.9"\n'
        )
        return root / "scripts" / "release" / "check-version.sh"

    def test_release_and_prerelease_tags(self):
        script = self.script_with_version("1.2.3")
        self.assertEqual(run([script, "--print"]).stdout, "1.2.3\n")
        result = run([script, "--tag", "v1.2.3"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "version=1.2.3\nprerelease=false\n")
        script = self.script_with_version("1.2.4-rc.1")
        result = run([script, "--tag", "v1.2.4-rc.1"])
        self.assertEqual(result.stdout, "version=1.2.4-rc.1\nprerelease=true\n")

    def test_rejects_mismatched_or_malformed_tags(self):
        script = self.script_with_version("1.2.3")
        for tag in ["v1.2.4", "1.2.3", "v1.2", "v01.2.3", "v1.2.3-", "release"]:
            result = run([script, "--tag", tag])
            self.assertNotEqual(result.returncode, 0, tag)

    def test_writes_github_output(self):
        script = self.script_with_version("0.4.0")
        output = Path(self.enterContext(tempfile.TemporaryDirectory())) / "out"
        run([script, "--tag", "v0.4.0"], env={**os.environ, "GITHUB_OUTPUT": str(output)})
        self.assertEqual(output.read_text(), "version=0.4.0\nprerelease=false\n")


class Publish(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.dist = self.root / "dist"
        stage = self.root / "stage" / "plenty-0.4.0-x86_64-unknown-linux-gnu"
        stage.mkdir(parents=True)
        (stage / "README.md").write_text("# Plenty 0.4.0\n\n## Install\n")
        self.dist.mkdir()
        archive = "plenty-0.4.0-x86_64-unknown-linux-gnu.tar.gz"
        subprocess.run(["tar", "-czf", self.dist / archive, "-C", stage.parent, stage.name], check=True)
        subprocess.run(f"sha256sum {archive} > {archive}.sha256", shell=True, cwd=self.dist, check=True)
        self.checksum = self.dist / f"{archive}.sha256"
        engine = stage.parent / "plenty-dataframe-0.4.0-x86_64-unknown-linux-gnu"
        engine.mkdir()
        (engine / "README.md").write_text("# Dataframe engine\n")
        engine_archive = f"{engine.name}.tar.gz"
        subprocess.run(["tar", "-czf", self.dist / engine_archive, "-C", engine.parent, engine.name], check=True)
        subprocess.run(f"sha256sum {engine_archive} > {engine_archive}.sha256", shell=True, cwd=self.dist, check=True)
        self.engine_checksum = self.dist / f"{engine_archive}.sha256"
        linalg = stage.parent / "plenty-linalg-0.4.0-x86_64-unknown-linux-gnu"
        linalg.mkdir()
        (linalg / "README.md").write_text("# Linear algebra engine\n")
        linalg_archive = f"{linalg.name}.tar.gz"
        subprocess.run(["tar", "-czf", self.dist / linalg_archive, "-C", linalg.parent, linalg.name], check=True)
        digest = hashlib.sha256((self.dist / linalg_archive).read_bytes()).hexdigest()
        self.linalg_checksum = self.dist / f"{linalg_archive}.sha256"
        self.linalg_checksum.write_text(f"{digest}  {linalg_archive}\n")
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        (bin_dir / "gh").write_text(FAKE_GH)
        (bin_dir / "gh").chmod(0o755)
        self.log = self.root / "gh.log"
        self.env = {
            **os.environ,
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "FAKE_GH_LOG": str(self.log),
            "GITHUB_REPOSITORY": "owner/plenty",
        }

    def publish(self, state, tag="v0.4.0", prerelease="false", published=None):
        self.log.write_text("")
        env = {**self.env, "FAKE_GH_STATE": state, "FAKE_GH_PUBLISHED": str(published or self.dist)}
        result = run([RELEASE / "publish.sh", tag, prerelease, self.dist], env=env)
        calls = [re.sub(r"--notes-file \S+", "--notes-file N", line) for line in self.log.read_text().splitlines()]
        return result, [c.split(" --repo")[0] for c in calls]

    def test_new_release_is_drafted_then_published(self):
        result, calls = self.publish("missing")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, ["gh release view v0.4.0", "gh release create v0.4.0 " + " ".join(
            str(p) for p in sorted(self.dist.iterdir())), "gh release edit v0.4.0"])
        self.assertIn("--draft --verify-tag", self.log.read_text())
        self.assertIn("--draft=false --latest=true", self.log.read_text())

    def test_prerelease_is_not_latest(self):
        result, _ = self.publish("missing", prerelease="true")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--draft=false --latest=false --prerelease", self.log.read_text())

    def test_interrupted_draft_is_resumed_without_a_second_release(self):
        result, calls = self.publish("draft")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([c.split(" ")[2] for c in calls], ["view", "upload", "edit", "edit"])
        self.assertIn("--clobber", self.log.read_text())

    def test_published_release_is_never_modified(self):
        result, calls = self.publish("published")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([c.split(" ")[2] for c in calls], ["view", "download"])
        other = self.root / "other.sha256"
        other.write_text("0" * 64 + "  plenty-0.4.0-x86_64-unknown-linux-gnu.tar.gz\n")
        published = self.root / "published"
        published.mkdir()
        other.rename(published / self.checksum.name)
        shutil.copy(self.engine_checksum, published)
        shutil.copy(self.linalg_checksum, published)
        result, calls = self.publish("published", published=published)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([c.split(" ")[2] for c in calls], ["view", "download"])

    def test_published_engine_must_also_match(self):
        published = self.root / "published"
        published.mkdir()
        shutil.copy(self.checksum, published)
        shutil.copy(self.linalg_checksum, published)
        for contents in [None, "different engine checksum\n"]:
            if contents:
                (published / self.engine_checksum.name).write_text(contents)
            result, calls = self.publish("published", published=published)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual([c.split(" ")[2] for c in calls], ["view", "download"])

    def test_published_linalg_must_also_match(self):
        published = self.root / "published"
        published.mkdir()
        shutil.copy(self.checksum, published)
        shutil.copy(self.engine_checksum, published)
        for contents in [None, "different linalg checksum\n"]:
            if contents:
                (published / self.linalg_checksum.name).write_text(contents)
            result, calls = self.publish("published", published=published)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual([c.split(" ")[2] for c in calls], ["view", "download"])

    def test_incomplete_or_mismatched_assets_never_publish(self):
        result, calls = self.publish("missing", tag="v0.4.1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])
        self.engine_checksum.unlink()
        result, calls = self.publish("missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])

    def test_all_target_archives_are_required_and_published_together(self):
        import io
        for component in ("plenty", "plenty-dataframe", "plenty-linalg"):
            name = f"{component}-0.4.0-aarch64-unknown-linux-gnu"
            archive = self.dist / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as package:
                text = b"# ARM release fixture\n"
                entry = tarfile.TarInfo(f"{name}/README.md")
                entry.size = len(text)
                package.addfile(entry, io.BytesIO(text))
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")
        self.env["PLENTY_RELEASE_TARGETS"] = "x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu"
        result, calls = self.publish("missing")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("plenty-0.4.0-aarch64-unknown-linux-gnu.tar.gz", calls[1])
        self.assertIn("plenty-0.4.0-x86_64-unknown-linux-gnu.tar.gz", calls[1])
        (self.dist / "plenty-linalg-0.4.0-aarch64-unknown-linux-gnu.tar.gz").unlink()
        result, calls = self.publish("missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])

    def test_notes_include_optional_engine_and_both_checksums(self):
        result = run([RELEASE / "release-notes.sh", self.dist])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(self.checksum.read_text().strip(), result.stdout)
        self.assertIn(self.engine_checksum.read_text().strip(), result.stdout)
        self.assertIn(self.linalg_checksum.read_text().strip(), result.stdout)
        self.assertIn("optional shared library", result.stdout)

    def native_archives(self, version="0.4.0"):
        import io
        import zipfile
        for target, ext in [("aarch64-apple-darwin", "tar.gz"), ("x86_64-pc-windows-msvc", "zip")]:
            name = f"plenty-{version}-{target}"
            archive = self.dist / f"{name}.{ext}"
            text = f"# Plenty for {target} (experimental)\n\n## Install\n".encode()
            if ext == "zip":
                with zipfile.ZipFile(archive, "w") as package:
                    package.writestr(f"{name}/README.md", text)
            else:
                with tarfile.open(archive, "w:gz") as package:
                    entry = tarfile.TarInfo(f"{name}/README.md")
                    entry.size = len(text)
                    package.addfile(entry, io.BytesIO(text))
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")

    def test_native_os_targets_publish_only_the_core_archive(self):
        self.native_archives()
        self.env["PLENTY_RELEASE_TARGETS"] = "x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-pc-windows-msvc"
        result, calls = self.publish("missing")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("plenty-0.4.0-x86_64-pc-windows-msvc.zip.sha256", calls[1])
        self.assertIn("plenty-0.4.0-aarch64-apple-darwin.tar.gz", calls[1])
        self.assertNotIn("plenty-dataframe-0.4.0-aarch64-apple-darwin", calls[1])
        (self.dist / "plenty-0.4.0-x86_64-pc-windows-msvc.zip").unlink()
        result, calls = self.publish("missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])

    def test_unlisted_archive_blocks_publication(self):
        self.native_archives()
        result, calls = self.publish("missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected archive/checksum set", result.stderr)
        self.assertEqual(calls, [])

    def test_notes_cover_each_target_in_order(self):
        self.native_archives()
        env = {**os.environ, "PLENTY_RELEASE_TARGETS":
               "x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-pc-windows-msvc"}
        result = run([RELEASE / "release-notes.sh", self.dist], env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        out = result.stdout
        positions = [out.index(f"Download `plenty-0.4.0-{t}") for t in
                     ("x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-pc-windows-msvc")]
        self.assertEqual(positions, sorted(positions))
        self.assertIn("## Plenty for x86_64-pc-windows-msvc (experimental)", out)
        self.assertIn("**Experimental:** macOS 11 or later", out)
        self.assertIn((self.dist / "plenty-0.4.0-x86_64-pc-windows-msvc.zip.sha256").read_text().strip(), out)
        self.assertEqual(out.count("Experimental:**"), 2)

    def test_unknown_lookup_failure_creates_nothing(self):
        result, calls = self.publish("broken")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, ["gh release view v0.4.0"])


class Prerequisites(unittest.TestCase):
    """The archive README, the installation page, and the smoke containers
    must install the same packages."""

    @staticmethod
    def commands(path):
        return [line for line in path.read_text().splitlines() if line.startswith("sudo ")]

    def test_readme_and_book_list_the_same_commands(self):
        readme = self.commands(RELEASE / "README.dist.md")
        self.assertEqual(len(readme), 3)
        self.assertEqual(readme, self.commands(INSTALL_PAGE))

    def test_smoke_containers_install_the_documented_packages(self):
        smoke = (RELEASE / "smoke-in-container.sh").read_text()
        for command in self.commands(RELEASE / "README.dist.md"):
            install = command.split("&&")[-1]
            words = install.split()
            packages = [w for w in words[3:] if not w.startswith("-")]
            manager = words[1]
            line = next(l for l in smoke.splitlines() if f"{manager} " in l and "prerequisites=" in l)
            for package in packages:
                self.assertRegex(line, rf"\s{re.escape(package)}(\s|')", command)

    def test_tested_distributions_are_the_smoke_images(self):
        env = (RELEASE / "toolchain.env").read_text()
        images = re.search(r'SMOKE_IMAGES="([^"]*)"', env).group(1).split()
        names = {"almalinux": "AlmaLinux", "debian": "Debian", "ubuntu": "Ubuntu",
                 "fedora": "Fedora", "archlinux": "Arch Linux"}
        for doc in [RELEASE / "README.dist.md", INSTALL_PAGE]:
            text = " ".join(doc.read_text().split())
            for image in images:
                name, version = re.match(r"([^:]+):([^@]+)", image).groups()
                self.assertIn(names[name], text, doc)
                if version != "latest":
                    self.assertRegex(text, rf"{names[name]} [^.]*{re.escape(version.split('.')[0])}", doc)



class TargetElfAudit(unittest.TestCase):
    """Exercise target identity independently of host binutils support."""
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        readelf = self.root / "readelf"
        readelf.write_text('''#!/bin/sh
case "$1" in
-h) printf "Class: %s\\nData: 2's complement, %s endian\\nMachine: %s\\nType: DYN\\n" "${TEST_CLASS:-ELF64}" "${TEST_ENDIAN:-little}" "${TEST_MACHINE:-AArch64}" ;;
-l) echo "[Requesting program interpreter: ${TEST_LOADER:-/lib/ld-linux-aarch64.so.1}]" ;;
-d) echo '(NEEDED) Shared library: [libc.so.6]' ;;
-V) echo "GLIBC_2.17 GCC_${TEST_GCC:-4.2.0}" ;;
esac
''')
        readelf.chmod(0o755)
        self.env = {**os.environ, "PATH": f"{self.root}:{os.environ['PATH']}",
                    "PLENTY_RELEASE_TARGET": "aarch64-unknown-linux-gnu"}

    def audit(self, **changes):
        return run([RELEASE / "check-elf.sh", "fixture"], env={**self.env, **changes})

    def test_arm_loader_and_machine_are_accepted(self):
        result = self.audit()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_mixed_architecture_loader_word_size_endian_and_gcc_are_rejected(self):
        for changes, message in [({"TEST_MACHINE": "Advanced Micro Devices X86-64"}, "not a aarch64"),
                                 ({"TEST_LOADER": "/lib64/ld-linux-x86-64.so.2"}, "interpreter"),
                                 ({"TEST_CLASS": "ELF32"}, "ELF64"),
                                 ({"TEST_ENDIAN": "big"}, "little-endian"),
                                 ({"TEST_GCC": "4.8.0"}, "newer than GCC_4.2.0")]:
            with self.subTest(changes=changes):
                result = self.audit(**changes)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn(message, result.stderr)


class ReleaseMatrices(unittest.TestCase):
    def test_two_native_targets_and_arm_baseline_current_smokes(self):
        result = run(["python3", "-B", RELEASE / "matrix.py"])
        self.assertEqual(result.returncode, 0, result.stderr)
        values = dict(line.split("=", 1) for line in result.stdout.splitlines())
        builds = json.loads(values["build_matrix"])["include"]
        self.assertEqual({row["runner"] for row in builds}, {"ubuntu-24.04", "ubuntu-24.04-arm"})
        self.assertEqual(len({row["target"] for row in builds}), 2)
        self.assertTrue(all("@sha256:" in row["image"] for row in builds))
        smokes = json.loads(values["smoke_matrix"])["include"]
        arm = [row for row in smokes if row["target"].startswith("aarch64")]
        self.assertEqual({row["image"].split("@", 1)[0] for row in arm}, {"almalinux:8", "debian:13"})
        self.assertTrue(all(row["runner"] == "ubuntu-24.04-arm" for row in arm))


    def test_native_targets_build_on_their_own_runners(self):
        result = run(["python3", "-B", RELEASE / "matrix.py"])
        values = dict(line.split("=", 1) for line in result.stdout.splitlines())
        native = json.loads(values["native_matrix"])["include"]
        self.assertEqual([(r["target"], r["runner"], r["ext"], r["components"]) for r in native],
                         [("aarch64-apple-darwin", "macos-15", "tar.gz", "plenty"),
                          ("x86_64-pc-windows-msvc", "windows-2025", "zip", "plenty")])
        self.assertEqual(values["targets"].split(), [
            "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
            "aarch64-apple-darwin", "x86_64-pc-windows-msvc"])
        self.assertEqual((values["linux"], values["native"]), ("true", "true"))

    def test_dry_run_selection_limits_jobs_and_targets(self):
        for select, linux, native in [("linux", "true", "false"), ("native", "false", "true")]:
            with self.subTest(select=select):
                result = run(["python3", "-B", RELEASE / "matrix.py", "--select", select])
                self.assertEqual(result.returncode, 0, result.stderr)
                values = dict(line.split("=", 1) for line in result.stdout.splitlines())
                self.assertEqual((values["linux"], values["native"]), (linux, native))
                builds = json.loads(values["build_matrix"])["include"]
                rows = json.loads(values["native_matrix"])["include"]
                self.assertEqual(sorted(values["targets"].split()),
                                 sorted([r["target"] for r in builds] + [r["target"] for r in rows]))
                if select == "native":
                    self.assertEqual(json.loads(values["smoke_matrix"])["include"], [])
        self.assertNotEqual(run(["python3", "-B", RELEASE / "matrix.py", "--select", "mac"]).returncode, 0)

    def test_workflow_publishes_only_from_tag_pushes(self):
        workflow = (REPO / ".github" / "workflows" / "release.yml").read_text()
        publish = workflow[workflow.index("\n  publish:"):]
        self.assertIn("github.event_name == 'push'", publish)
        self.assertIn("needs.plan.outputs.targets", publish)
        self.assertIn("workflow_dispatch:", workflow)
        for uses in re.findall(r"uses: (\S+)", workflow):
            self.assertRegex(uses, r"@[0-9a-f]{40}$", uses)


class TargetFacts(unittest.TestCase):
    def facts(self, target):
        script = ('. "$1"; printf "%s|%s|%s|%s|%s|%s" "$release_os" "$release_cpu" "$release_exe" '
                  '"$release_archive_ext" "$release_components" "$release_experimental"')
        return run(["bash", "-c", script, "facts", RELEASE / "target.sh"],
                   env={**os.environ, "PLENTY_RELEASE_TARGET": target})

    def test_native_os_targets(self):
        expected = {
            "x86_64-unknown-linux-gnu": "linux|x86-64||tar.gz|plenty plenty-dataframe plenty-linalg|0",
            "aarch64-unknown-linux-gnu": "linux|generic||tar.gz|plenty plenty-dataframe plenty-linalg|0",
            "aarch64-apple-darwin": "macos|apple-m1||tar.gz|plenty|1",
            "x86_64-pc-windows-msvc": "windows|x86-64|.exe|zip|plenty|1",
        }
        for target, facts in expected.items():
            with self.subTest(target=target):
                result = self.facts(target)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, facts)

    def test_unknown_target_fails(self):
        result = self.facts("x86_64-apple-darwin")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported release target", result.stderr)

    def test_os_readmes_name_their_archives_and_prerequisites(self):
        for os_name, needles in [("macos", ["macOS 11", "xcode-select --install", "com.apple.quarantine",
                                             "@TARGET@.tar.gz", "experimental"]),
                                 ("windows", ["Windows 10", "Build Tools", "@TARGET@.zip",
                                              "plenty.exe", "experimental"])]:
            text = (RELEASE / f"README.dist-{os_name}.md").read_text()
            for needle in needles:
                self.assertIn(needle, text, os_name)
            self.assertNotIn("glibc", text)


class Archive(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        stage = self.root / "stage"
        self.name = "plenty-0.4.0-x86_64-pc-windows-msvc"
        (stage / self.name / "bin").mkdir(parents=True)
        (stage / self.name / "licenses" / "rust").mkdir(parents=True)
        (stage / self.name / "bin" / "plenty.exe").write_bytes(b"MZ exe")
        (stage / self.name / "README.md").write_text("# Plenty\n")
        (stage / self.name / "licenses" / "rust" / "COPYRIGHT.html").write_text("rust")
        (stage / self.name / "README.md").chmod(0o600)
        self.stage = stage

    def build(self, out, epoch="1700000000"):
        return run(["python3", "-B", RELEASE / "archive.py", self.stage, self.name, out],
                   env={**os.environ, "SOURCE_DATE_EPOCH": epoch})

    def test_archives_are_reproducible_with_fixed_modes(self):
        import zipfile
        for ext in ("zip", "tar.gz"):
            with self.subTest(ext=ext):
                first, second = self.root / f"a.{ext}", self.root / f"b.{ext}"
                self.assertEqual(self.build(first).returncode, 0)
                os.utime(self.stage / self.name / "README.md", (1, 1))
                self.assertEqual(self.build(second).returncode, 0)
                self.assertEqual(first.read_bytes(), second.read_bytes())
                if ext == "zip":
                    with zipfile.ZipFile(first) as archive:
                        modes = {i.filename: (i.external_attr >> 16) & 0o7777 for i in archive.infolist()}
                        names = archive.namelist()
                        self.assertEqual(archive.read(f"{self.name}/README.md"), b"# Plenty\n")
                else:
                    with tarfile.open(first) as archive:
                        members = archive.getmembers()
                        modes = {m.name.rstrip("/") + ("/" if m.isdir() else ""): m.mode for m in members}
                        names = [m.name + ("/" if m.isdir() else "") for m in members]
                        self.assertTrue(all(m.uid == 0 and m.gid == 0 and m.mtime == 1700000000 for m in members))
                self.assertEqual(names, sorted(names))
                self.assertEqual(modes[f"{self.name}/bin/plenty.exe"], 0o755)
                self.assertEqual(modes[f"{self.name}/README.md"], 0o644)
                self.assertEqual(modes[f"{self.name}/licenses/"], 0o755)

    def test_unknown_format_fails(self):
        self.assertNotEqual(self.build(self.root / "a.7z").returncode, 0)


class CheckNative(unittest.TestCase):
    """Synthetic Mach-O and PE headers exercise the audit without Apple or
    Microsoft tools."""

    def setUp(self):
        spec = importlib.util.spec_from_file_location("check_native", RELEASE / "check-native.py")
        self.check = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.check)

    @staticmethod
    def macho(dylibs=("/usr/lib/libSystem.B.dylib",), minos=(11, 0), cpu=0x0100000C,
              signed=True, rpath=False):
        import struct
        commands = []
        for name in dylibs:
            raw = name.encode() + b"\0"
            raw += b"\0" * (-(24 + len(raw)) % 8)
            commands.append(struct.pack("<IIIIII", 0xC, 24 + len(raw), 24, 2, 0, 0) + raw)
        if minos:
            commands.append(struct.pack("<IIIIII", 0x32, 24, 1, (minos[0] << 16) | (minos[1] << 8), 0, 0))
        if rpath:
            raw = b"@loader_path\0\0\0\0"
            commands.append(struct.pack("<III", 0x8000001C, 12 + len(raw), 12) + raw)
        if signed:
            commands.append(struct.pack("<IIII", 0x1D, 16, 0, 0))
        body = b"".join(commands)
        return struct.pack("<IiIIIIII", 0xFEEDFACF, cpu, 0, 2, len(commands), len(body), 0, 0) + body

    @staticmethod
    def pe(dlls=("KERNEL32.dll", "VCRUNTIME140.dll", "api-ms-win-crt-runtime-l1-1-0.dll"),
           machine=0x8664, subsystem=3):
        import struct
        optional = bytearray(240)
        struct.pack_into("<H", optional, 0, 0x20B)
        struct.pack_into("<H", optional, 68, subsystem)
        struct.pack_into("<I", optional, 108, 16)
        section_rva, raw = 0x1000, 0x400
        table = bytearray(20 * (len(dlls) + 1))
        names = bytearray()
        for i, dll in enumerate(dlls):
            struct.pack_into("<I", table, 20 * i + 12, section_rva + len(table) + len(names))
            names += dll.encode() + b"\0"
        data = bytes(table + names)
        struct.pack_into("<II", optional, 112 + 8, section_rva, len(table))
        coff = struct.pack("<HHIIIHH", machine, 1, 0, 0, 0, len(optional), 0x22)
        section = struct.pack("<8sIIIIIIHHI", b".idata", len(data), section_rva, len(data), raw, 0, 0, 0, 0, 0)
        header = bytearray(0x40)
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 0x3C, 0x40)
        image = bytes(header) + b"PE\0\0" + coff + bytes(optional) + section
        return image + b"\0" * (raw - len(image)) + data

    def test_macos_baseline_executable_is_accepted(self):
        problems, summary = self.check.check_macho(self.macho(), "11.0")
        self.assertEqual(problems, [])
        self.assertIn("macOS 11.0.0+", summary)
        frameworks = ("/usr/lib/libSystem.B.dylib", "/System/Library/Frameworks/Security.framework/Versions/A/Security")
        self.assertEqual(self.check.check_macho(self.macho(dylibs=frameworks), "11.0")[0], [])

    def test_macos_runner_dependencies_are_rejected(self):
        for kwargs, message in [({"dylibs": ("/opt/homebrew/lib/libz.1.dylib",)}, "not a macOS system library"),
                                ({"minos": (15, 0)}, "needs macOS 15.0.0"),
                                ({"minos": None}, "no minimum macOS version"),
                                ({"cpu": 0x01000007}, "expected arm64"),
                                ({"signed": False}, "no code signature"),
                                ({"rpath": True}, "LC_RPATH")]:
            with self.subTest(kwargs=kwargs):
                problems, _ = self.check.check_macho(self.macho(**kwargs), "11.0")
                self.assertTrue(any(message in p for p in problems), problems)
        self.assertIn("fat", self.check.check_macho(b"\xca\xfe\xba\xbe" + b"\0" * 40, "11.0")[0][0])

    def test_windows_system_and_msvc_runtime_imports_are_accepted(self):
        problems, summary = self.check.check_pe(self.pe())
        self.assertEqual(problems, [])
        self.assertIn("VCRUNTIME140.dll", summary)

    def test_windows_foreign_imports_and_headers_are_rejected(self):
        for kwargs, message in [({"dlls": ("KERNEL32.dll", "libgcc_s_seh-1.dll")}, "imports libgcc_s_seh-1.dll"),
                                ({"dlls": ("zlib1.dll",)}, "imports zlib1.dll"),
                                ({"machine": 0xAA64}, "expected x86-64"),
                                ({"subsystem": 2}, "expected console")]:
            with self.subTest(kwargs=kwargs):
                problems, _ = self.check.check_pe(self.pe(**kwargs))
                self.assertTrue(any(message in p for p in problems), problems)

    def test_linux_executable_is_neither(self):
        elf = b"\x7fELF" + b"\0" * 60
        self.assertTrue(self.check.check_macho(elf, "11.0")[0])
        self.assertTrue(self.check.check_pe(elf)[0])


class SmokeNative(unittest.TestCase):
    """Run smoke-native.py on Linux against a macOS-named archive whose
    executables are shell stand-ins; the real run happens on macos-15."""

    FAKE = """#!/bin/sh
case "$1" in
--version) echo "$(basename "$0") 0.4.0 (aarch64-apple-darwin)" ;;
--check) test -f "$2" ;;
--compile) printf '#!/bin/sh\\nexec "%s" run\\n' "$0" > "$4"; chmod +x "$4" ;;
*) printf '%s' "$SMOKE_EXPECTED" ;;
esac
"""

    def setUp(self):
        spec = importlib.util.spec_from_file_location("smoke_native", RELEASE / "smoke-native.py")
        self.smoke = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.smoke)
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.name = "plenty-0.4.0-aarch64-apple-darwin"
        stage = self.root / "stage"
        top = stage / self.name
        (top / "bin").mkdir(parents=True)
        (top / "licenses" / "rust").mkdir(parents=True)
        for entry in self.smoke.REQUIRED:
            (top / entry).write_text(entry)
        for tool in ("plenty", "plentyc"):
            (top / "bin" / tool).write_text(self.FAKE)
        self.stage = stage
        self.archive = self.root / "dist" / f"{self.name}.tar.gz"
        self.archive.parent.mkdir()

    def pack(self, **env):
        subprocess.run(["python3", "-B", RELEASE / "archive.py", self.stage, self.name, self.archive], check=True)
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.archive.with_name(self.archive.name + ".sha256").write_text(f"{digest}  {self.archive.name}\n")
        return run(["python3", "-B", RELEASE / "smoke-native.py", self.archive],
                   env={**os.environ, "SMOKE_EXPECTED": env.get("expected", self.smoke.EXPECTED)})

    def test_relocatable_archive_passes(self):
        result = self.pack()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("PASS plenty-0.4.0-aarch64-apple-darwin", result.stdout)

    def test_wrong_output_missing_file_and_bad_checksum_fail(self):
        result = self.pack(expected="wrong\n")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("printed 'wrong\\n'", result.stderr)
        (self.stage / self.name / "LICENSE-MIT").unlink()
        result = self.pack()
        self.assertIn("missing plenty-0.4.0-aarch64-apple-darwin/LICENSE-MIT", result.stderr)
        self.archive.with_name(self.archive.name + ".sha256").write_text("0" * 64 + f"  {self.archive.name}\n")
        result = run(["python3", "-B", RELEASE / "smoke-native.py", self.archive])
        self.assertIn("checksum file does not match", result.stderr)


if __name__ == "__main__":
    unittest.main()
