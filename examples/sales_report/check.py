#!/usr/bin/env python3
"""Compile and exercise the real application; only Python's stdlib is needed."""

import argparse
from collections import defaultdict
import csv
from datetime import datetime
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
ORDER_FIELDS = ["order_id", "occurred_at", "product_id", "amount_cents"]


def read_csv(path):
    with Path(path).open(newline="", encoding="utf-8") as source:
        return list(csv.DictReader(source))


def write_csv(path, fields, rows):
    with Path(path).open("w", newline="", encoding="utf-8") as output:
        writer = csv.DictWriter(output, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def oracle(rows, products):
    categories = {p["product_id"]: p["category"].lower() for p in products}
    groups = defaultdict(lambda: [0, 0])
    rejected = []
    for row in rows:
        try:
            stamp = datetime.strptime(row["occurred_at"], "%Y-%m-%d %H:%M:%S")
        except ValueError:
            stamp = None
        try:
            cents = int(row["amount_cents"])
        except ValueError:
            cents = -1
        if not row["order_id"]:
            reason = "missing_order_id"
        elif stamp is None:
            reason = "invalid_timestamp"
        elif not 0 <= cents <= 1_000_000_000:
            reason = "invalid_amount"
        elif row["product_id"] not in categories:
            reason = "unknown_product"
        else:
            group = groups[(stamp.date().isoformat(), categories[row["product_id"]])]
            group[0] += 1
            group[1] += cents
            continue
        rejected.append({**row, "reason": reason})
    daily = [dict(day=day, category=category, lines=str(n), total_cents=str(cents))
             for (day, category), (n, cents) in sorted(groups.items())]
    # CSV has no nullable type; the application sorts null IDs last.
    rejected.sort(key=lambda r: (not bool(r["order_id"]), r["order_id"], r["occurred_at"]))
    return daily, rejected


def invoke(command, **kwargs):
    return subprocess.run([str(arg) for arg in command], capture_output=True, text=True,
                          timeout=120, **kwargs)


class SalesReportTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.storage = tempfile.TemporaryDirectory(prefix="plenty-sales-check-")
        cls.addClassCleanup(cls.storage.cleanup)
        cls.root = Path(cls.storage.name)
        cls.app = cls.root / "sales-report"
        cls.convert = cls.root / "convert"
        cls.environment = {**os.environ, "PLENTY_CACHE": "off", "POLARS_MAX_THREADS": "2"}
        cls.compile_times = {}
        for name, destination in [("main", cls.app), ("convert", cls.convert)]:
            start = time.perf_counter()
            result = invoke([OPTIONS.plenty, "--compile", HERE / f"{name}.plenty", "-o", destination],
                            cwd=cls.root, env=cls.environment)
            if result.returncode:
                raise RuntimeError(result.stdout + result.stderr)
            cls.compile_times[name] = time.perf_counter() - start

    def setUp(self):
        self.case = self.root / self._testMethodName
        self.case.mkdir()
        self.rows = read_csv(HERE / "data/orders-01.csv") + read_csv(HERE / "data/orders-02.csv")
        self.products = read_csv(HERE / "data/products.csv")
        self.orders_path = self.case / "orders.csv"
        self.products_path = self.case / "products.csv"
        self.output = self.case / "output"
        self.output.mkdir()
        self.save()

    def save(self):
        write_csv(self.orders_path, ORDER_FIELDS, self.rows)
        write_csv(self.products_path, ["product_id", "category"], self.products)

    def run_report(self, kind="csv", path=None, success=True):
        result = invoke([self.app, kind, path or self.orders_path, self.products_path, self.output],
                        cwd=self.case, env=self.environment)
        if success:
            self.assertEqual(result.returncode, 0, result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout)
        return result

    def assert_outputs(self):
        daily, rejected = oracle(self.rows, self.products)
        self.assertEqual(read_csv(self.output / "daily.csv"), daily)
        self.assertEqual(read_csv(self.output / "rejected.csv"), rejected)
        inspected = self.case / "inspected.csv"
        result = invoke([self.convert, "inspect", self.output / "daily.parquet", inspected],
                        cwd=self.case, env=self.environment)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(read_csv(inspected), daily)

    def test_csv_report(self):
        result = self.run_report()
        self.assertEqual(result.stdout, "input lines: 12\naccepted lines: 5\nrejected lines: 7\ndaily groups: 4\n")
        self.assert_outputs()

    def test_partitioned_parquet(self):
        for index in [1, 2]:
            result = invoke([self.convert, "ingest", HERE / f"data/orders-0{index}.csv",
                             self.case / f"part-{index}.parquet"], env=self.environment)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.run_report("parquet", self.case / "part-*.parquet")
        self.assert_outputs()

    def test_empty_input(self):
        self.rows = []
        self.save()
        self.run_report()
        self.assert_outputs()

    def test_all_rejected(self):
        self.rows = [dict(order_id="bad", occurred_at="bad", product_id="001", amount_cents="bad"),
                     dict(order_id="", occurred_at="bad", product_id="", amount_cents="bad")]
        self.save()
        self.run_report()
        self.assert_outputs()

    def test_boundaries_nulls_and_unicode(self):
        self.products.append(dict(product_id="004", category="Café, 雪"))
        self.rows += [dict(order_id=f"extra-{i}", occurred_at="2024-03-01 23:59:59",
                           product_id="004", amount_cents=value)
                      for i, value in enumerate(["1000000000", "9223372036854775808", "0", "NaN"])]
        self.rows += [dict(order_id="no-date", occurred_at="", product_id="001", amount_cents="10"),
                      dict(order_id="no-product", occurred_at="2024-03-01 00:00:00", product_id="", amount_cents="10")]
        self.save()
        self.run_report()
        self.assert_outputs()

    def test_duplicate_catalogue(self):
        self.products.append(self.products[0])
        self.save()
        result = self.run_report(success=False)
        self.assertIn("must be unique", result.stderr)
        self.assertEqual(list(self.output.iterdir()), [])

    def test_missing_category(self):
        self.products[0]["category"] = ""
        self.save()
        self.run_report(success=False)
        self.assertEqual(list(self.output.iterdir()), [])

    def test_empty_catalogue(self):
        self.products = []
        self.save()
        self.run_report()
        self.assert_outputs()

    def test_quoted_empty_catalogue_key(self):
        self.products_path.write_text('product_id,category\n"",books\n', encoding="utf-8")
        result = self.run_report(success=False)
        self.assertIn("must not be empty", result.stderr)

    def test_missing_input_column(self):
        write_csv(self.orders_path, ["order_id"], [dict(order_id="x")])
        result = self.run_report(success=False)
        self.assertIn("missing required order column", result.stderr)

    def test_missing_input_file(self):
        self.run_report(path=self.case / "absent.csv", success=False)

    def test_missing_output_directory(self):
        self.output.rmdir()
        self.run_report(success=False)

    def test_existing_outputs_unchanged(self):
        self.run_report()
        before = {p.name: p.read_bytes() for p in self.output.iterdir()}
        self.run_report(success=False)
        self.assertEqual({p.name: p.read_bytes() for p in self.output.iterdir()}, before)

    def test_cli(self):
        self.assertEqual(invoke([self.app, "--help"], env=self.environment).returncode, 0)
        self.assertEqual(invoke([self.app], env=self.environment).returncode, 2)
        result = invoke([self.app, "unknown", self.orders_path, self.products_path, self.output], env=self.environment)
        self.assertEqual(result.returncode, 2)

    def test_scaled_workload(self):
        if not OPTIONS.benchmark_rows:
            self.skipTest("pass --benchmark-rows N to measure the complete CSV application")
        self.rows = [dict(order_id=f"line-{i}", occurred_at=f"2024-03-{1 + i % 28:02d} 12:00:00",
                          product_id=f"{1 + i % 3:03d}", amount_cents=str(i % 10000))
                     for i in range(OPTIONS.benchmark_rows)]
        self.save()
        times, peaks = [], []
        for repeat in range(3):
            self.output = self.case / f"repeat-{repeat}"
            self.output.mkdir()
            metrics = self.case / f"time-{repeat}.txt"
            result = invoke(["/usr/bin/time", "-f", "%e %M", "-o", metrics, self.app, "csv",
                             self.orders_path, self.products_path, self.output], env=self.environment)
            self.assertEqual(result.returncode, 0, result.stderr)
            elapsed, rss = metrics.read_text().split()
            times.append(float(elapsed))
            peaks.append(int(rss))
        self.assert_outputs()
        print(json.dumps(dict(rows=len(self.rows), polars_threads=2, elapsed_seconds=times,
                              median_seconds=statistics.median(times), max_rss_kib=max(peaks),
                              compile_seconds_cache_off=self.compile_times,
                              executable_bytes=self.app.stat().st_size), indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plenty", type=Path, default=ROOT / "target/debug/plenty")
    parser.add_argument("--benchmark-rows", type=int, default=0)
    OPTIONS, remaining = parser.parse_known_args()
    OPTIONS.plenty = OPTIONS.plenty.resolve()
    if OPTIONS.benchmark_rows < 0:
        parser.error("--benchmark-rows must be nonnegative")
    unittest.main(argv=[__file__, *remaining])
