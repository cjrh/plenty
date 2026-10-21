"""Tests for scripts/release: version checks, publication states, and the
consistency of the documented prerequisites with what the smoke test installs.

The archive build and smoke test themselves run through
scripts/release/local-check.sh, which needs Docker; see
book/src/development/releasing.md.
"""

import os
import importlib.util
import re
import shutil
import subprocess
import tempfile
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
  "release download") cp "$FAKE_GH_PUBLISHED" "${@: -1}/" ;;
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
        env = {**self.env, "FAKE_GH_STATE": state, "FAKE_GH_PUBLISHED": str(published or self.checksum)}
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
        other = other.rename(self.root / self.checksum.name)
        result, calls = self.publish("published", published=other)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual([c.split(" ")[2] for c in calls], ["view", "download"])

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


if __name__ == "__main__":
    unittest.main()
