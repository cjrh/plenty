"""Guard native job independence, release cache compatibility and qualification gates."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parent.parent


def job(workflow, name):
    return re.search(rf"^  {name}:\n(.*?)(?=^  [a-z][a-z-]*:|\Z)",
                     workflow, re.M | re.S).group(1)


class NativeCiWorkflow(unittest.TestCase):
    def setUp(self):
        self.native = (ROOT / ".github/workflows/portability.yml").read_text()
        self.release = (ROOT / ".github/workflows/release.yml").read_text()

    def test_stable_release_and_engine_preparation_are_independent(self):
        for name in ("compiler", "release-compiler", "dataframe"):
            with self.subTest(job=name):
                self.assertNotIn("needs:", job(self.native, name))
        stable = job(self.native, "compiler")
        for check in ("native/tools.rs", "native/ring_sources.rs", "platform-tests/Cargo.toml",
                      "cargo build --locked --workspace --timings",
                      "cargo test --locked --workspace --lib", "test_arm_stack_probes::"):
            self.assertIn(check, stable)
        self.assertNotIn("--phase prepare", stable)
        pinned = job(self.native, "release-compiler")
        self.assertIn("--test --phase prepare", pinned)
        self.assertIn("native-compiler-${{ matrix.target }}", pinned)
        self.assertIn("--validate", job(self.native, "dataframe"))
        self.assertIn("needs: [compiler, release-compiler, dataframe]", job(self.native, "package"))
        self.assertIn("smoke-native.py", job(self.native, "package"))

    def test_windows_checkouts_match_release_lf_policy_before_hashing(self):
        for name in ("compiler", "release-compiler", "dataframe", "package"):
            body = job(self.native, name)
            self.assertLess(body.index("git config --global core.autocrlf false"),
                            body.index("uses: actions/checkout@"))
        for name in ("native-build", "native-engine"):
            body = job(self.release, name)
            self.assertLess(body.index("git config --global core.autocrlf false"),
                            body.index("uses: actions/checkout@"))

    def test_shared_cache_policy_and_valid_cargo_color_match(self):
        for workflow in (self.native, self.release):
            self.assertIn("\nenv:\n  CARGO_TERM_COLOR: always\n", workflow)
            # Empty CARGO_TERM_COLOR breaks cargo metadata inside rust-cache.
            self.assertNotRegex(workflow, r'CARGO_TERM_COLOR:\s*[\'\"]{2}')
        for key in re.findall(r"shared-key: (.+)", self.release):
            self.assertIn("shared-key: " + key, self.native)
        for name in ("release-compiler", "dataframe"):
            self.assertIn("save-if: ${{ github.ref == 'refs/heads/main' }}", job(self.native, name))

    def test_report_waits_for_all_qualification_jobs_even_on_failure(self):
        body = job(self.native, "timings")
        self.assertIn("if: always()", body)
        self.assertIn("needs: [compiler, release-compiler, dataframe, package]", body)
        self.assertIn("actions: read", body)
        self.assertIn("scripts/check_timings.py ci", body)
        self.assertIn("retention-days: 30", body)
