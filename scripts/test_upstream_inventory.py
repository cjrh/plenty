import json
import unittest

import upstream_inventory as inventory


class AcknowledgementTests(unittest.TestCase):
    def test_bullets_end_at_the_next_heading(self):
        page = (
            "# m\n\ntext\n\n## Acknowledgements\n\n"
            "- Wraps [SQLite](https://sqlite.org/) 3.54.0, vendored\n  in the runtime (public domain).\n"
            "- `Builder` is modelled on Go's [`strings.Builder`](https://pkg.go.dev/strings).\n"
            "\n## Later\n- not an acknowledgement\n"
        )
        self.assertEqual(inventory.acknowledgements(page), [
            "Wraps [SQLite](https://sqlite.org/) 3.54.0, vendored in the runtime (public domain).",
            "`Builder` is modelled on Go's [`strings.Builder`](https://pkg.go.dev/strings).",
        ])
        self.assertEqual(inventory.acknowledgements("# none\n"), [])

    def test_kinds_and_versions(self):
        self.assertEqual(inventory.ack_kind("Wraps zlib-rs 0.6.8"), "wrapped")
        self.assertEqual(inventory.ack_kind("Parser ported from commonmark.js"), "ported")
        self.assertEqual(inventory.ack_kind("Modelled on Go's net/http"), "modelled")
        self.assertEqual(
            inventory.VERSION.findall("0.1.25 (MIT OR Apache-2.0), Unicode 17.0.0 (Unicode-3.0), tzdata 2026f"),
            ["0.1.25", "17.0.0"],
        )


    def test_items_pair_each_link_with_its_version(self):
        bullet = ("Wraps the RustCrypto [`sha2`](https://x/hashes) 0.10.9 and [`hmac`](https://x/macs) "
                  "0.12.1 crates (MIT OR Apache-2.0).")
        self.assertEqual(inventory.ack_items(bullet), [
            ("sha2", "https://x/hashes", "0.10.9"), ("hmac", "https://x/macs", "0.12.1")])
        self.assertEqual(inventory.ack_items("Wraps the [Brotli](https://b) C codec 1.2.1 (MIT)."),
                         [("Brotli", "https://b", "1.2.1")])
        self.assertEqual(inventory.ack_items("Modelled on [heapq](https://h)."), [("heapq", "https://h", "")])

    def test_drift_compares_wrapped_versions_with_pins(self):
        entries = [
            {"group": "crypto-tls", "kind": "crate", "name": "ring", "pinned": "0.17.14", "where": "Cargo.lock"},
            {"group": "acknowledgements", "kind": "wrapped", "name": "ring", "pinned": "0.17.13", "where": "tls.md"},
            {"group": "acknowledgements", "kind": "wrapped", "name": "ring", "pinned": "0.17.14", "where": "crypto.md"},
            {"group": "acknowledgements", "kind": "modelled", "name": "ring", "pinned": "0.1.0", "where": "x.md"},
        ]
        self.assertEqual(inventory.drift(entries), [
            {"name": "ring", "acknowledged": "0.17.13", "pinned": ["0.17.14"], "where": "tls.md"}])


class DenyTests(unittest.TestCase):
    def test_parse_deny_reads_diagnostics_only(self):
        lines = [
            json.dumps({"type": "log", "fields": {"message": "no config"}}),
            "not json",
            json.dumps({"type": "diagnostic", "fields": {
                "advisory": {"id": "RUSTSEC-2026-0001", "package": "zstd", "title": "Leak",
                             "informational": None, "aliases": ["CVE-2026-1"]},
                "labels": [{"span": "zstd 0.13.3 registry+https://github.com/rust-lang/crates.io-index"}],
                "notes": ["ID: RUSTSEC-2026-0001", "Solution: Upgrade to >=0.14.3"],
            }}),
            json.dumps({"type": "summary", "fields": {"advisories": {"errors": 1}}}),
        ]
        self.assertEqual(inventory.parse_deny(lines, "Cargo.lock"), [{
            "lockfile": "Cargo.lock", "id": "RUSTSEC-2026-0001", "package": "zstd 0.13.3",
            "title": "Leak", "kind": "vulnerability", "aliases": ["CVE-2026-1"],
            "solution": "Upgrade to >=0.14.3",
        }])


class RepositoryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.entries = inventory.inventory(tz_repo="/nonexistent")

    def find(self, name, kind=None):
        return [e for e in self.entries if e["name"] == name and (kind is None or e["kind"] == kind)]

    def test_every_direct_dependency_is_locked(self):
        crates = [e for e in self.entries if e["kind"] == "crate"]
        self.assertGreater(len(crates), 20)
        for entry in crates:
            self.assertNotEqual(entry["pinned"], "not locked", entry)

    def test_named_upstreams_are_present(self):
        self.assertEqual(self.find("cranelift-codegen")[0]["group"], "codegen")
        self.assertEqual(self.find("regalloc2")[0]["group"], "codegen")
        self.assertEqual(self.find("polars")[0]["group"], "dataframe")
        self.assertIn("git+", self.find("faer")[0]["source"])
        self.assertEqual(self.find("sqlite", "vendored C")[0]["group"], "sqlite")
        self.assertEqual(self.find("brotli", "vendored C")[0]["group"], "compression")
        self.assertRegex(self.find("mdbook")[0]["pinned"], r"^\d+\.\d+\.\d+$")
        self.assertRegex(self.find("Unicode Character Database", "data")[0]["pinned"], r"^\d+\.\d+\.\d+$")
        self.assertRegex(self.find("tz package (plentylang/tz)")[0]["pinned"], r"^v\d")
        self.assertEqual(self.find("IANA tzdata")[0]["pinned"], "unknown")
        self.assertTrue(any(e["group"] == "acknowledgements" for e in self.entries))

    def test_acknowledged_versions_match_pins(self):
        self.assertEqual(inventory.drift(self.entries), [])

    def test_every_group_has_a_researchable_entry(self):
        groups = {e["group"] for e in self.entries}
        self.assertLessEqual(set(inventory.GROUPS), groups)


if __name__ == "__main__":
    unittest.main()
