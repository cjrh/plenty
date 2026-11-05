"""Unit tests for bench.py: python3 -B -m unittest discover -s benchmarks/game -p 'test_*.py'"""

import io
import unittest
from contextlib import redirect_stdout

import bench


class OutputComparison(unittest.TestCase):
    def test_exact(self):
        self.assertTrue(bench.outputs_match(b"a 1\n", b"a 1\n", "exact"))
        self.assertFalse(bench.outputs_match(b"a 1\n", b"a 1", "exact"))

    def test_float_tolerance(self):
        self.assertTrue(bench.outputs_match(b"-0.169075164\n", b"-0.169075165\n", "float:1e-8"))
        self.assertFalse(bench.outputs_match(b"-0.16907\n", b"-0.16908\n", "float:1e-8"))
        self.assertFalse(bench.outputs_match(b"1 2\n", b"1\n", "float:1e-8"))


class Manifests(unittest.TestCase):
    def test_every_manifest_is_complete(self):
        for manifest in bench.load_manifests():
            with self.subTest(manifest["name"]):
                self.assertIn(manifest["status"], ("ready", "blocked"))
                reference = manifest["reference"]
                self.assertTrue((manifest["dir"] / reference["file"]).is_file())
                self.assertTrue((manifest["dir"] / manifest["check"]["expected"]).is_file())
                self.assertIn(manifest["input"], ("argument", "fasta-stdin"))
                self.assertEqual(set(manifest["sizes"]), {"quick", "full", "site"})
                if manifest["status"] == "blocked":
                    self.assertTrue(manifest["blocked_reason"])
                    self.assertEqual(manifest["plenty"], [])
                for variant in manifest["plenty"]:
                    self.assertTrue((manifest["dir"] / variant["file"]).is_file())

    def test_unknown_benchmark_is_an_error(self):
        with self.assertRaises(bench.BenchError):
            bench.load_manifests(["no-such-benchmark"])


def results(median, size=10, instructions=None):
    record = {"median_s": median, "ratio_to_reference": median / 1.0}
    if instructions:
        record["instructions"] = instructions
    return {
        "mode": "quick",
        "benchmarks": {
            "x": {"status": "ready", "size": size, "programs": {"reference": {"median_s": 1.0}, "plenty-idiomatic": record}},
        },
    }


class Compare(unittest.TestCase):
    def run_compare(self, baseline, current, **kwargs):
        with redirect_stdout(io.StringIO()) as out:
            status = bench.compare(baseline, current, kwargs.get("threshold", 0.1), kwargs.get("metric", "time"))
        return status, out.getvalue()

    def test_regression_beyond_threshold_fails(self):
        status, out = self.run_compare(results(1.0), results(1.2))
        self.assertEqual(status, 1)
        self.assertIn("REGRESSION", out)

    def test_change_within_threshold_passes(self):
        status, _ = self.run_compare(results(1.0), results(1.05))
        self.assertEqual(status, 0)

    def test_different_size_is_skipped(self):
        status, out = self.run_compare(results(1.0, size=10), results(9.0, size=20))
        self.assertEqual(status, 0)
        self.assertIn("skipped", out)

    def test_instruction_metric(self):
        status, _ = self.run_compare(results(1.0, instructions=100), results(1.0, instructions=150), metric="instructions")
        self.assertEqual(status, 1)


if __name__ == "__main__":
    unittest.main()
