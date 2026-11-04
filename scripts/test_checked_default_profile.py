"""Pure Python synthetic hotpath reports/mock processes; no compiler execution."""
import base64
import copy
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import checked_default_profile as profile


def encoded(value):
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()


def synthetic_report(metric="count", label="job", role="main", extra=()):
    names = list(profile.MINIMUM_PHASES) + list(extra)
    alloc, timing = [], []
    for i, name in enumerate(names):
        loc = profile.phase_locations(role).get(name, {"file": "src/extra.rs", "line": 1, "column": 4})
        row = {"id": i, "name": name, "calls": 2, "sampled_calls": 2,
               "total": "10" if metric == "count" else "2.0 KB",
               "avg": "5" if metric == "count" else "1.0 KB",
               "p50": "5" if metric == "count" else "1.0 KB",
               "p95": "5" if metric == "count" else "1.0 KB", "percent_total": "20.00%"}
        if name != "main":
            row["location"] = loc
        alloc.append(row)
        time = {**row, "total": "2.00 ms", "avg": "1.00 ms", "p50": "1.00 ms", "p95": "1.00 ms"}
        timing.append(time)
    common = {"time_elapsed": "3.00 ms", "total_elapsed_ns": 3000000,
              "caller_name": "main", "percentiles": [50.0, 95.0],
              "included_count": len(names), "total_count": len(names)}
    return {"type": "hotpath_report", "version": "0.28.6", "label": label,
            "meta": {"rustc": "synthetic", "os": "linux-x86_64", "created_at": "2000-01-01T00:00:00Z"},
            "functions_alloc": {**common, "profiling_mode": "alloc-" + metric,
                "description": f"Exclusive allocation {metric} by each function (excluding nested calls).",
                "total_allocated": str(10 * len(names)) if metric == "count" else f"{2 * len(names)}.0 KB", "data": alloc},
            "functions_timing": {**common, "profiling_mode": "timing",
                "description": "Function execution time metrics.", "data": timing}}


def impossible_report(kind, metric="count", label="job", role="main"):
    """Keep all five phases/calls/sections: only the reviewed defect is changed."""
    if kind == "byte-average":
        metric = "bytes"
    data = synthetic_report(metric, label, role)
    if kind == "byte-average":
        for row in data["functions_alloc"]["data"]:
            row.update(total="2 B", avg="0 B", p50="1 B", p95="1 B")
        data["functions_alloc"]["total_allocated"] = "10 B"
    elif kind == "duration-average":
        for row in data["functions_timing"]["data"]:
            row.update(total="0 ns", avg="999.00 s", p50="1 ns", p95="1 ns")
    elif kind == "meta":
        data["meta"] = {}
    else:
        raise AssertionError(kind)
    return data


def displayed_duration(ns):
    """Synthetic mirror of output.rs format_duration; not profiler evidence."""
    if ns < 1000:
        return f"{ns} ns"
    factor, unit = ((1000, "µs") if ns < 1000000 else
                    (1000000, "ms") if ns < 1000000000 else (1000000000, "s"))
    return f"{float(ns) / factor:.2f} {unit}"


def displayed_bytes(value):
    # The large-u64 tests use the producer's capped TB band, avoiding a mock
    # logarithm-based unit-selection claim at exact power-of1024 boundaries.
    return f"{float(value) / 1024 ** 4:.1f} TB" if value >= 1024 ** 4 else f"{value} B"


class ReportTests(unittest.TestCase):
    def parse(self, data=None, metric="count", **kwargs):
        return profile.parse_report(encoded(data or synthetic_report(metric)), metric, "job", **kwargs)

    def test_count_totals_are_exact_and_not_calls(self):
        parsed = self.parse()
        value = parsed["sections"]["functions_alloc"]["main"]
        self.assertEqual(value["value"], {"precision": "EXACT", "total_count_exact": 10})
        self.assertEqual(value["raw"]["calls"], 2)

    def test_approximate_bytes_keep_raw_and_binary_unit(self):
        parsed = self.parse(metric="bytes")
        value = parsed["sections"]["functions_alloc"]["main"]["value"]
        self.assertEqual(value, {"raw": "2.0 KB", "precision": "APPROXIMATE", "nominal_bytes_approx": "2048.0"})
        self.assertNotIn("exact_bytes", json.dumps(parsed))

    def test_byte_units_are_all_source_verified_powers_of_1024(self):
        for i, unit in enumerate(("B", "KB", "MB", "GB", "TB")):
            with self.subTest(unit=unit):
                value = "1 B" if i == 0 else "1.0 " + unit
                self.assertEqual(DecimalString(profile.byte_value(value)["nominal_bytes_approx"]), 1024 ** i)

    def test_non_display_byte_strings_rejected(self):
        for value in ("1 KiB", "1 KB", "1.00 KB", "1.0 B", "1024 B", "-1 B", "NaN KB", "inf GB", "1e3 B", "01 B", "0.0 MB", "184467440737095516160.0 TB"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                profile.byte_value(value)

    def test_rounding_does_not_turn_nominal_values_into_exact_totals(self):
        self.assertEqual(profile.byte_value("1024.0 KB")["precision"], "APPROXIMATE")
        self.assertEqual(profile.byte_interval("2.0 KB"), (DecimalString("1996.8"), DecimalString("2099.2")))

    def test_count_strings_reject_short_rounded_or_malformed(self):
        for value in ("1 K", "1.0", "01", "1,000", "+1", "-1", " 1", "1\n", 1, True, "N/A", "18446744073709551616"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                profile.count_value(value)

    def test_duplicate_json_keys_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate JSON"):
            profile.parse_report(b'{"type":"hotpath_report","type":"hotpath_report"}', "count", "job")

    def test_truncated_malformed_nonfinite_json_rejected(self):
        for raw in (b"", b'{"type":', b'{"x":NaN}', encoded(synthetic_report())[:-8]):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                profile.parse_report(raw, "count", "job")

    def test_nonobject_report_or_rows_rejected(self):
        with self.assertRaises(ValueError):
            profile.parse_report(b"[]", "count", "job")
        data = synthetic_report()
        data["functions_alloc"]["data"][0] = []
        with self.assertRaises(ValueError):
            self.parse(data)

    def test_metric_version_label_type_confusion_rejected(self):
        for key, value in (("type", "other"), ("version", "0.28.7"), ("label", "other")):
            data = synthetic_report()
            data[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.parse(data)
        with self.assertRaises(ValueError):
            self.parse(synthetic_report("bytes"))

    def test_sections_required_even_with_exit_zero(self):
        for key in ("functions_alloc", "functions_timing", "meta"):
            data = synthetic_report()
            del data[key]
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.parse(data)

    def test_complete_included_total_and_rows_required(self):
        for section in ("functions_alloc", "functions_timing"):
            for field, value in (("included_count", 4), ("total_count", 6), ("total_count", True), ("data", [])):
                data = synthetic_report()
                data[section][field] = value
                with self.subTest(section=section, field=field), self.assertRaises(ValueError):
                    self.parse(data)

    def test_missing_minimum_phase_rejected(self):
        data = synthetic_report()
        for section in ("functions_alloc", "functions_timing"):
            data[section]["data"].pop()
            data[section]["included_count"] = data[section]["total_count"] = 4
        data["functions_alloc"]["total_allocated"] = "40"
        with self.assertRaisesRegex(ValueError, "minimum"):
            self.parse(data)

    def test_duplicate_phase_name_or_id_rejected(self):
        for field in ("name", "id"):
            data = synthetic_report()
            data["functions_alloc"]["data"][1][field] = data["functions_alloc"]["data"][0][field]
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "duplicate"):
                self.parse(data)

    def test_short_phase_names_rejected(self):
        data = synthetic_report()
        data["functions_alloc"]["data"][1]["name"] = "program_object"
        with self.assertRaisesRegex(ValueError, "short phase"):
            self.parse(data)

    def test_counts_totals_and_calls_must_be_consistent_nonnegative(self):
        for key, value in (("calls", -1), ("calls", 0), ("calls", True), ("sampled_calls", 1),
                           ("avg", "6"), ("total", "-1"), ("percent_total", "101.00%")):
            data = synthetic_report()
            data["functions_alloc"]["data"][0][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                self.parse(data)
        data = synthetic_report()
        data["functions_alloc"]["total_allocated"] = "51"
        with self.assertRaisesRegex(ValueError, "exclusive count total"):
            self.parse(data)

    def test_approximate_totals_and_calls_consistency(self):
        for key, value in (("total_allocated", "1.0 KB"), ("total_allocated", "500.0 TB")):
            data = synthetic_report("bytes")
            data["functions_alloc"][key] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                self.parse(data, "bytes")
        data = synthetic_report("bytes")
        data["functions_alloc"]["data"][0]["avg"] = "500.0 GB"
        with self.assertRaisesRegex(ValueError, "average/calls"):
            self.parse(data, "bytes")

    def test_complete_five_phase_byte_integer_average_boundary_rejected(self):
        data = impossible_report("byte-average")
        self.assertEqual(len(data["functions_alloc"]["data"]), 5)
        self.assertEqual(data["functions_alloc"]["total_allocated"], "10 B")
        with self.assertRaisesRegex(ValueError, "approximate total/average/calls"):
            self.parse(data, "bytes")

    def test_complete_five_phase_duration_average_counterexample_rejected(self):
        for average in ("999.00 s", "999 s"):
            data = impossible_report("duration-average")
            for row in data["functions_timing"]["data"]:
                row["avg"] = average
            with self.subTest(average=average), self.assertRaises(ValueError):
                self.parse(data)
        with self.assertRaisesRegex(ValueError, "duration total/average/sampled_calls"):
            self.parse(impossible_report("duration-average"))

    def test_exact_integer_average_truncation_bounds_bytes_and_nanoseconds(self):
        for section, metric, suffix in (("functions_alloc", "bytes", "B"), ("functions_timing", "count", "ns")):
            for total, average, valid in ((0, 0, True), (1, 0, True), (2, 0, False), (2, 1, True),
                                          (3, 1, True), (4, 1, False), (1, 1, False)):
                data = synthetic_report(metric)
                for row in data[section]["data"]:
                    row.update(total=f"{total} {suffix}", avg=f"{average} {suffix}", p50=f"1 {suffix}", p95=f"1 {suffix}")
                if metric == "bytes":
                    data[section]["total_allocated"] = f"{total * 5} B"
                with self.subTest(section=section, total=total, average=average):
                    if valid:
                        self.parse(data, metric)
                    else:
                        with self.assertRaises(ValueError):
                            self.parse(data, metric)

    def test_duration_format_bands_rounding_and_integer_bounds(self):
        # Ties, unit transitions, and large u64 rounding stay conservative.
        for ns in (0, 1, 999, 1000, 1005, 1015, 999994, 999995, 999999,
                   1000000, 1005000, 1015000, 999999999, 1000000000,
                   1005000000, 1015000000, (1 << 64) - 1):
            with self.subTest(ns=ns):
                lo, hi = profile.duration_value(displayed_duration(ns))
                self.assertLessEqual(lo, ns)
                self.assertGreaterEqual(hi, ns)
        self.assertEqual(profile.duration_value("999 ns"), (999, 999))
        for raw in ("1000 ns", "1 µs", "1.000 ms", "0.00 s", "0.99 µs", "999 s", "NaN s"):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                profile.duration_value(raw)

    def test_zero_totals_accept_clamped_timing_histograms(self):
        for metric in ("count", "bytes"):
            data = synthetic_report(metric)
            for row in data["functions_alloc"]["data"]:
                row.update(total="0" if metric == "count" else "0 B",
                           avg="0" if metric == "count" else "0 B",
                           p50="0" if metric == "count" else "0 B", p95="0" if metric == "count" else "0 B")
            data["functions_alloc"]["total_allocated"] = "0" if metric == "count" else "0 B"
            for row in data["functions_timing"]["data"]:
                # alloc/state.rs clamps duration histogram samples to at least1,
                # but sums the actual zero duration. Do not derive sums from HDR.
                row.update(total="0 ns", avg="0 ns", p50="1 ns", p95="1 ns")
            result = self.parse(data, metric)
            self.assertIn("not recovered raw timing", result["sections"]["functions_timing"]["main"]["value"]["display_precision"])

    def test_rounded_duration_average_uses_possible_raw_integer_not_display_quotient(self):
        data = synthetic_report()
        for row in data["functions_timing"]["data"]:
            # Real integers: total1005ns /2 truncates to502ns. The total's
            # displayed microseconds are rounded; percentile rounding is independent.
            row.update(total=displayed_duration(1005), avg="502 ns", p50="503 ns", p95="503 ns")
        self.parse(data)
        for row in data["functions_timing"]["data"]:
            row["avg"] = "600 ns"
        with self.assertRaises(ValueError):
            self.parse(data)

    def test_large_u64_local_ieee_bounds_accept_real_boundary_and_reject_impossible(self):
        maximum = (1 << 64) - 1
        # Near a decimal half-step, f64 casting can move a large integer across
        # the ideal Decimal interval. These are synthetic producer-format checks.
        near_byte_tie = int(DecimalString("16777215.95") * 1024 ** 4)
        near_time_tie = 18446744073705000001
        for value in (maximum, near_byte_tie - 512, near_byte_tie + 512):
            with self.subTest(bytes=value):
                lo, hi = profile.byte_integer_bounds(displayed_bytes(value))
                self.assertLessEqual(lo, value)
                self.assertGreaterEqual(hi, value)
                data = synthetic_report("bytes")
                for row in data["functions_alloc"]["data"]:
                    row.update(total="0 B", avg="0 B", p50="0 B", p95="0 B")
                first = data["functions_alloc"]["data"][0]
                first.update(total=displayed_bytes(value), avg=displayed_bytes(value // 2), p50="953.7 MB", p95="953.7 MB")
                data["functions_alloc"]["total_allocated"] = displayed_bytes(value)
                self.parse(data, "bytes")
                first["avg"] = displayed_bytes(value // 2 + 1024 ** 4)
                with self.assertRaises(ValueError):
                    self.parse(data, "bytes")
        for value in (maximum, near_time_tie - 1024, near_time_tie + 1024):
            with self.subTest(nanoseconds=value):
                data = synthetic_report()
                for row in data["functions_timing"]["data"]:
                    row.update(total=displayed_duration(value), avg=displayed_duration(value // 2))
                self.parse(data)
                for row in data["functions_timing"]["data"]:
                    row["avg"] = displayed_duration(value // 2 + 20000000)
                with self.assertRaises(ValueError):
                    self.parse(data)
        # The local IEEE bound never weakens exact small-unit checks.
        self.assertEqual(profile.byte_integer_bounds("2 B"), (2, 2))
        self.assertEqual(profile.duration_value("2 ns"), (2, 2))
        self.assertEqual(profile.duration_value("1.00 µs"), (1000, 1005))

    def test_meta_required_fields_and_json_types(self):
        for meta in ({}, None, [], "metadata"):
            data = synthetic_report()
            data["meta"] = meta
            with self.subTest(meta=meta), self.assertRaises(ValueError):
                self.parse(data)
        for key in ("rustc", "os", "created_at"):
            for invalid in (None, 1, True, [], {}):
                data = synthetic_report()
                data["meta"][key] = invalid
                with self.subTest(key=key, invalid=invalid), self.assertRaises(ValueError):
                    self.parse(data)
            data = synthetic_report()
            del data["meta"][key]
            with self.subTest(missing=key), self.assertRaises(ValueError):
                self.parse(data)

    def test_meta_rfc3339_calendar_time_offset_validity(self):
        for value in ("", "not-a-time", "2000-01-01", "2000-01-01T00:00:00", "2000-01-01T00:00:00+0000",
                      "2000-01-01T00:00:00Z trailing", "2000-01-01T00:00:00.Z", "2001-02-29T00:00:00Z",
                      "2000-13-01T00:00:00Z", "2000-04-31T00:00:00Z", "2000-01-00T00:00:00Z",
                      "2000-01-01T24:00:00Z", "2000-01-01T00:60:00Z", "2000-01-01T00:00:61Z",
                      "2000-01-01T00:00:00+24:00", "2000-01-01T00:00:00+00:60", "2000-01-01T00:00:60Z"):
            data = synthetic_report()
            data["meta"]["created_at"] = value
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "created_at"):
                self.parse(data)

    def test_valid_meta_optional_fields_and_timestamp_retained_unchanged(self):
        for value in ("2000-02-29T00:00:00Z", "2026-11-04T12:34:56.123456789+05:30",
                      "2000-01-01t00:00:00z", "0000-02-29T00:00:00Z", "9999-12-31T23:59:59-00:00",
                      "2016-12-31T23:59:60Z", "2017-01-01T00:59:60+01:00"):
            data = synthetic_report()
            # rustc can legitimately be empty when build-time version lookup failed.
            data["meta"].update(rustc="", created_at=value, source_root="", optional_future={"keep": [1, None]})
            original = copy.deepcopy(data["meta"])
            self.assertEqual(self.parse(data)["raw"]["meta"], original)

    def test_cumulative_and_excluded_wrapper_rejected(self):
        data = synthetic_report()
        data["functions_alloc"]["description"] = "Cumulative allocation count during each function call (including nested calls)."
        with self.assertRaisesRegex(ValueError, "attribution"):
            self.parse(data)
        data = synthetic_report()
        data["functions_alloc"]["caller_name"] = "not-main"
        with self.assertRaises(ValueError):
            self.parse(data)

    def test_sampling_rejected(self):
        data = synthetic_report()
        data["time_sampling"] = {"functions": 0.5}
        with self.assertRaisesRegex(ValueError, "sampled"):
            self.parse(data)

    def test_location_and_source_version_provenance(self):
        data = synthetic_report()
        result = self.parse(data, expected_locations=profile.phase_locations("main"))
        self.assertEqual(result["sections"]["functions_alloc"]["plenty::frontend::lower"]["raw"]["location"]["line"], 4732)
        with self.assertRaisesRegex(ValueError, "pinned source"):
            self.parse(data, expected_locations=profile.phase_locations("candidate"))

    def test_partial_alloc_timing_sets_or_calls_rejected(self):
        for key, value in (("calls", 3), ("location", {"file": "other", "line": 1, "column": 1})):
            data = synthetic_report()
            data["functions_timing"]["data"][1][key] = value
            if key == "calls":
                data["functions_timing"]["data"][1]["sampled_calls"] = 3
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.parse(data)

    def test_timing_is_inclusive_not_summed_as_latency(self):
        data = synthetic_report()
        data["functions_timing"]["data"][1]["percent_total"] = "200.00%"
        result = self.parse(data)
        comparison = profile.compare_reports(result, result)
        self.assertNotIn("functions_timing", comparison)
        self.assertTrue(any("not latency" in s for s in result["limits"]))

    def test_added_removed_phases_preserved_and_ratios_only_common(self):
        a = self.parse(synthetic_report(extra=("old::phase",)))
        b = self.parse(synthetic_report(extra=("new::phase",)))
        result = profile.compare_reports(a, b)
        self.assertEqual(set(result["added_phases"]), {"new::phase"})
        self.assertEqual(set(result["removed_phases"]), {"old::phase"})
        self.assertEqual(len(result["common_phases"]), 5)
        self.assertEqual(result["common_phases"][0]["ratio"]["after_over_before"], "10/10")

    def test_incompatible_comparison_rejected(self):
        with self.assertRaises(ValueError):
            profile.compare_reports(self.parse(), self.parse(metric="bytes"))

    def test_zero_denominator_is_unavailable(self):
        data = synthetic_report()
        for row in data["functions_alloc"]["data"]:
            for field in ("total", "avg", "p50", "p95"):
                row[field] = "0"
        data["functions_alloc"]["total_allocated"] = "0"
        a = self.parse(data)
        self.assertIsNone(profile.compare_reports(a, self.parse())["common_phases"][0]["ratio"]["after_over_before"])

    def test_byte_ratio_is_labelled_approximate(self):
        result = profile.compare_reports(self.parse(metric="bytes"), self.parse(metric="bytes"))
        self.assertEqual(result["common_phases"][0]["ratio"]["precision"], "APPROXIMATE-byte-ratio")


# Avoid binary floats even in synthetic rounding expectations.
from decimal import Decimal as DecimalString


class AdapterFixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="checked-default-profile-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.native = self.root / "native"
        self.inputs = self.native / "inputs"
        self.inputs.mkdir(parents=True)
        self.records_path = self.root / "records.json"
        self.report_path, self.review_path = self.root / "report.md", self.root / "review.md"
        self.report_path.write_text("synthetic native report")
        self.review_path.write_text("synthetic native review")
        self.inventory = {"schema": 1, "entries": []}
        for mode in "TCIU":
            for units in (1000, 4000):
                path = self.inputs / f"sources/{mode}/corpus-{units}.plenty"
                path.parent.mkdir(exist_ok=True, parents=True)
                path.write_text(f"SYNTHETIC not a Plenty program {mode} {units}")
                self.inventory["entries"].append({"mode": mode, "source": str(path.relative_to(self.inputs)),
                    "sha256": profile.digest(path.read_bytes()), "cases": [{"args": [], "expected": "success", "name": "checksum",
                        "stdout": f"{(units-1)**2+6*(units-1)+62}\n"}]})
        self.write(self.inputs / "inventory.json", self.inventory)
        builds = {}
        self.binary_pins = {}
        for role in profile.SOURCE_PINS:
            source = self.root / (role + "-source")
            (source / "src").mkdir(parents=True)
            (source / "src/main.rs").write_text("synthetic rust source")
            binary = self.root / (role + "-frozen")
            binary.write_bytes(b"not an executable:" + role.encode())
            binary.chmod(0o555)
            archive = self.root / (role + ".a")
            archive.write_bytes(b"synthetic archive " + role.encode())
            self.binary_pins[role] = profile.digest(binary.read_bytes())
            builds[role] = {"source_sha": profile.SOURCE_PINS[role], "source_path": str(source), "target_dir": str(self.root / (role + "-target")),
                "compiler_profile": "release", "features": ["profile", "profile-alloc"], "rustflags": "", "runtime_rustflags": "",
                "target": "x86_64-unknown-linux-gnu", "status": "PASS", "exit_code": 0, "final_status": "", "staged_files": [],
                "frozen_compiler": {"path": str(binary), "sha256": self.binary_pins[role]},
                "inputs": [{"path": str(source / "src/main.rs"), "sha256": profile.digest((source / "src/main.rs").read_bytes())}],
                "runtime_archives": [{"path": str(archive), "sha256": profile.digest(archive.read_bytes())}]}
            builds[role]["command"] = ["cargo", "build", "--locked", "--offline", "--target", builds[role]["target"], "--target-dir", builds[role]["target_dir"],
                                           "--no-default-features", "--bin", "plenty", "--lib", "--message-format=json", "--release", "--features", "profile-alloc"]
        tool = self.root / "rustc"
        tool.write_bytes(b"nonexecutable synthetic tool")
        self.records = {"schema": 1, "status": "PASS", "builds": builds,
            "toolchain": {"rustc_vV": "synthetic rustc version", "cargo_vV": "synthetic cargo version"},
            "native_tools": {"clang": {"version": "synthetic clang"}, "mold": {"version": "synthetic mold"}},
            "toolchain_files": [{"path": str(tool), "sha256": profile.digest(tool.read_bytes())}]}
        self.write(self.records_path, self.records)
        native_rows, index = [], []
        for entry in self.inventory["entries"]:
            for generated in ("debug", "release"):
                executable = self.native / "corpora" / entry["mode"] / generated / Path(entry["source"]).stem
                executable.parent.mkdir(parents=True, exist_ok=True)
                executable.write_bytes(b"not native code:" + str(executable).encode())
                index.append({"path": str(executable), "sha256": profile.digest(executable.read_bytes())})
                for phase in ("check", "build", "runtime"):
                    stdout = entry["cases"][0]["stdout"] if phase == "runtime" else ""
                    native_rows.append({"source": entry["source"], "profile": generated, "phase": phase,
                        "mode": entry["mode"], "case": "checksum" if phase == "runtime" else None,
                        "command": [str(executable)] if phase == "runtime" else ["synthetic-compiler", phase],
                        "returncode": 0, "timeout": False, "spawn_error": None, "stdout": stdout, "stderr": "",
                        "stdout_base64": base64.b64encode(stdout.encode()).decode(), "stderr_base64": ""})
        results_path = self.native / "corpora/results.jsonl"
        results_path.write_bytes(b"".join(json.dumps(r).encode() + b"\n" for r in native_rows))
        self.write(self.native / "corpora/environment.json", {"synthetic": "native env"})
        self.write(self.native / "corpora-proof.json", [{**r, "group": "corpora"} for r in native_rows if r["phase"] == "runtime"])
        for path in (results_path, self.native / "corpora/environment.json", self.native / "corpora-proof.json"):
            index.append({"path": str(path), "sha256": profile.digest(path.read_bytes())})
        self.write(self.native / "artifact-index.json", index)
        self.pins = {"index_sha256": profile.digest((self.native / "artifact-index.json").read_bytes()),
                     "inventory_sha256": profile.digest((self.inputs / "inventory.json").read_bytes()),
                     "corpora_proof_sha256": profile.digest((self.native / "corpora-proof.json").read_bytes()),
                     "report_sha256": profile.digest(self.report_path.read_bytes()), "review_sha256": profile.digest(self.review_path.read_bytes())}
        for key, value in {"BUILD_RECORD": self.records_path, "BUILD_RECORD_SHA256": profile.digest(self.records_path.read_bytes()),
                           "NATIVE_ROOT": self.native, "INPUT_ROOT": self.inputs, "NATIVE_REPORT": self.report_path,
                           "NATIVE_REVIEW": self.review_path, "NATIVE_PINS": self.pins, "BINARY_PINS": self.binary_pins}.items():
            p = patch.object(profile, key, value)
            p.start()
            self.addCleanup(p.stop)
        self.manifest = {"schema": 1, "purpose": "measurement", "adapter": profile.ADAPTER, "quiet_machine_approved": True,
                         "builds": {r: profile.canonical_build(self.records, r) for r in builds},
                         "accepted_build_records": {"path": str(self.records_path), "sha256": profile.BUILD_RECORD_SHA256},
                         "native_evidence": {"root": str(self.native), **self.pins}, "environment": profile.machine_snapshot(),
                         "child_environment": profile.BASE_ENV, "toolchain_files": self.records["toolchain_files"],
                         "comparison_pairs": [["C", "I"], ["T", "U"]]}
        self.manifest_path = self.root / "manifest.json"
        self.save_manifest()
        self.spawned = []
        self.behavior = None

    def write(self, path, value):
        path.write_bytes(encoded(value))

    def save_manifest(self):
        self.write(self.manifest_path, self.manifest)
        self.manifest_hash = profile.digest(self.manifest_path.read_bytes())

    def load(self):
        return profile.load_profile_manifest(self.manifest_path, self.manifest_hash)

    def mock_popen(self, command, **kwargs):
        self.spawned.append((command, kwargs))
        stdout, stderr = kwargs["stdout"], kwargs["stderr"]
        status = 0
        if command[0] == "/usr/bin/git":
            role = "main" if "main-source" in command[2] else "candidate"
            if command[3:] == ["rev-parse", "HEAD"]:
                stdout.write((profile.SOURCE_PINS[role] + "\n").encode())
            elif command[3] == "ls-files":
                stdout.write(b"src/main.rs\0")
        else:
            env = kwargs["env"]
            role = "main" if command[0] == self.records["builds"]["main"]["frozen_compiler"]["path"] else "candidate"
            self.write(Path(env["HOTPATH_OUTPUT_PATH"]), synthetic_report(env["HOTPATH_ALLOC_METRIC"], env["HOTPATH_REPORT_LABEL"], role))
            blob = bytearray(64)
            blob[:6] = b"\x7fELF\x02\x01"
            blob[16:20] = b"\x01\x00\x3e\x00"
            Path(command[-1]).write_bytes(blob)
            stdout.write(b"synthetic stdout\xff")
            if self.behavior:
                status = self.behavior(command, kwargs)
        process = unittest.mock.Mock()
        process.pid = 987654321
        process.wait.return_value = status
        return process

    def run_mock(self, **kwargs):
        with patch.object(profile.subprocess, "Popen", side_effect=self.mock_popen):
            return profile.execute(self.manifest_path, self.manifest_hash, self.root / "output", allow_profile=True, **kwargs)


class ManifestTests(AdapterFixture):
    def test_role_local_raw_commands_not_cross_role_equality(self):
        self.assertNotEqual(self.manifest["builds"]["main"]["build_command"], self.manifest["builds"]["candidate"]["build_command"])
        self.load()

    def test_manifest_fields_each_bound_to_own_authenticated_record(self):
        for key in ("binary", "binary_sha256", "source_tree", "source_sha", "compiler_profile", "cargo_features", "cargo_flags", "rustflags", "runtime_rustflags", "target", "toolchain", "native_linker", "runtime_archive_sha256", "build_command"):
            with self.subTest(key=key):
                manifest = copy.deepcopy(self.manifest)
                self.manifest["builds"]["main"][key] = ["wrong"] if isinstance(manifest["builds"]["main"][key], list) else "wrong"
                self.save_manifest()
                with self.assertRaises(ValueError):
                    self.load()
                self.manifest = manifest
        self.save_manifest()

    def test_arbitrary_executable_path_normalization_forbidden(self):
        # In particular, a source-path substring inside executable argv is not normalized.
        self.manifest["builds"]["main"]["build_command"][0] += self.records["builds"]["main"]["source_path"]
        self.save_manifest()
        with self.assertRaises(ValueError):
            self.load()

    def test_false_quiet_purpose_schema_adapter_rejected(self):
        for key, value in (("quiet_machine_approved", False), ("quiet_machine_approved", 1),
                           ("purpose", "syntax-correctness-only"), ("schema", 2), ("adapter", "other")):
            with self.subTest(key=key, value=value):
                original = self.manifest[key]
                self.manifest[key] = value
                self.save_manifest()
                with self.assertRaises(ValueError):
                    self.load()
                self.manifest[key] = original
        self.save_manifest()

    def test_parent_manifest_hash_cannot_be_amended(self):
        self.manifest_path.write_bytes(self.manifest_path.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "pinned hash"):
            self.load()

    def test_replaced_accepted_build_record_refused(self):
        self.records_path.write_bytes(self.records_path.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "pinned hash"):
            self.load()

    def test_native_pin_changes_even_self_consistent_rejected(self):
        self.manifest["native_evidence"]["index_sha256"] = "0" * 64
        self.save_manifest()
        with self.assertRaises(ValueError):
            self.load()

    def test_toolchain_and_environment_must_match_actual(self):
        for key, value in (("toolchain_files", []), ("environment", {}), ("child_environment", {"PLENTY_PROFILE": "0"})):
            original = self.manifest[key]
            self.manifest[key] = value
            self.save_manifest()
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.load()
            self.manifest[key] = original
        self.save_manifest()

    def test_only_approved_complete_pairs_accepted(self):
        for pairs in ([], [["T", "I"], ["C", "I"]], [["C", "I"], ["C", "I"]], [["C", "I"]]):
            self.manifest["comparison_pairs"] = pairs
            self.save_manifest()
            with self.subTest(pairs=pairs), self.assertRaises(ValueError):
                self.load()

    def test_opt_in_refused_before_any_process(self):
        with patch.object(profile.subprocess, "Popen") as popen, self.assertRaises(ValueError):
            profile.execute(self.manifest_path, self.manifest_hash, self.root / "output")
        popen.assert_not_called()
        self.assertFalse((self.root / "output").exists())

    def test_bad_timeout_refused(self):
        for timeout in (0, -1, float("nan"), float("inf"), True):
            with self.subTest(timeout=timeout), self.assertRaises(ValueError):
                profile.execute(self.manifest_path, self.manifest_hash, self.root / "output", True, timeout)


class NativeBindingTests(AdapterFixture):
    def test_original_full_native_qualifiers_imported_outside_interval(self):
        result = profile.native_qualifiers(self.inventory)
        self.assertEqual(len(result["rows"]), 48)
        self.assertIn("outside-profile-interval", result["kind"])

    def test_same_output_replaced_native_executable_refused(self):
        executable = next((self.native / "corpora/T/debug").iterdir())
        executable.write_bytes(b"replacement that could print same output")
        with self.assertRaisesRegex(ValueError, "pinned hash"):
            profile.native_qualifiers(self.inventory)

    def test_amended_index_cannot_bless_replaced_executable(self):
        index_path = self.native / "artifact-index.json"
        index = json.loads(index_path.read_text())
        executable = Path(index[0]["path"])
        executable.write_bytes(b"replacement")
        index[0]["sha256"] = profile.digest(executable.read_bytes())
        self.write(index_path, index)
        with self.assertRaisesRegex(ValueError, "pinned hash"):
            profile.native_qualifiers(self.inventory)

    def test_replaced_same_outcome_proof_refused(self):
        path = self.native / "corpora-proof.json"
        data = json.loads(path.read_text())
        data[0]["command"] = ["same-output-replacement"]
        self.write(path, data)
        with self.assertRaisesRegex(ValueError, "pinned hash"):
            profile.native_qualifiers(self.inventory)

    def test_review_and_report_are_pinned(self):
        for path in (self.review_path, self.report_path):
            original = path.read_bytes()
            path.write_bytes(b"new review or report")
            with self.subTest(path=path), self.assertRaises(ValueError):
                profile.native_qualifiers(self.inventory)
            path.write_bytes(original)

    def test_inventory_hash_binds_entire_input_not_just_selected_rows(self):
        self.inventory["unselected-new-field"] = "changed"
        self.write(self.inputs / "inventory.json", self.inventory)
        with self.assertRaisesRegex(ValueError, "pinned hash"):
            profile.accepted_inventory()

    def test_changed_source_refused(self):
        (self.inputs / self.inventory["entries"][0]["source"]).write_text("changed")
        with self.assertRaises(ValueError):
            profile.accepted_inventory()

    def test_symlink_source_refused_even_same_contents(self):
        source = self.inputs / self.inventory["entries"][0]["source"]
        saved = self.root / "saved-source"
        source.rename(saved)
        source.symlink_to(saved)
        with self.assertRaises(ValueError):
            profile.accepted_inventory()

    def test_hardlink_source_alias_refused(self):
        source = self.inputs / self.inventory["entries"][0]["source"]
        os.link(source, self.root / "alias")
        with self.assertRaises(ValueError):
            profile.accepted_inventory()


class PlanExecutionTests(AdapterFixture):
    def test_full_schedule_exactly_16_separate_metric_processes(self):
        plan = profile.make_plan(self.inventory, self.manifest["builds"], self.root / "planned")
        self.assertEqual(len(plan["jobs"]), 16)
        self.assertEqual(len({j["id"] for j in plan["jobs"]}), 16)
        self.assertEqual({j["phase"] for j in plan["jobs"]}, {"emit-object"})
        for entry in self.inventory["entries"]:
            self.assertEqual({j["metric"] for j in plan["jobs"] if j["source"].endswith(entry["source"])}, {"count", "bytes"})
        for job in plan["jobs"]:
            self.assertEqual(job["command"][1], "--emit-object")
            self.assertNotIn("--release", job["command"])
            self.assertEqual(job["source_version"], profile.SOURCE_PINS[job["role"]])
        self.assertIn("NOT-executed", plan["status"])

    def test_ambient_profile_even_zero_upload_preload_removed(self):
        ambient = {"PLENTY_PROFILE": "0", "PLENTY_PROFILE_OTHER": "yes", "HOTPATH_UPLOAD": "1", "HOTPATH_FOCUS": "x", "HOTPATH_FUNCTIONS_LIMIT": "1", "LD_PRELOAD": "wrong", "PLENTY_THREADS": "1"}
        with patch.dict(os.environ, ambient):
            job = profile.make_plan(self.inventory, self.manifest["builds"], self.root / "planned")["jobs"][0]
        env = job["environment"]
        self.assertEqual(env["PLENTY_PROFILE"], "1")
        self.assertEqual(env["HOTPATH_METRICS_SERVER_OFF"], "true")
        self.assertEqual(env["HOTPATH_FUNCTIONS_LIMIT"], "0")
        self.assertEqual(env["HOTPATH_ALLOC_CUMULATIVE"], "false")
        self.assertEqual(env["HOTPATH_FUNCTIONS_TIME_SAMPLING_RATE"], "1")
        self.assertFalse({"HOTPATH_UPLOAD", "HOTPATH_FOCUS", "PLENTY_PROFILE_OTHER", "LD_PRELOAD", "PLENTY_THREADS"} & env.keys())
        self.assertFalse(any("taskset" in part for part in job["command"]))

    def test_cli_plan_never_spawns(self):
        with patch.object(profile.subprocess, "Popen") as popen:
            self.assertEqual(profile.main(["plan", "--output", str(self.root / "planned")]), 0)
        popen.assert_not_called()
        self.assertEqual(len(json.loads((self.root / "planned/plan.json").read_text())["jobs"]), 16)

    def test_full_mock_pipeline_preserves_all_raw_records(self):
        result = self.run_mock()
        self.assertEqual(result["status"], "passed-bounded-compiler-profile-only", result.get("error"))
        self.assertEqual(len(result["jobs"]), 16)
        self.assertEqual(len(result["comparisons"]), 8)
        self.assertEqual(len(self.spawned), 30)  # 16 profiles + 12 source qualifiers + 2 source inventories.
        for row in result["jobs"]:
            self.assertEqual(row["status"], "passed")
            self.assertEqual(base64.b64decode(row["process"]["stdout_base64"]), b"synthetic stdout\xff")
            self.assertEqual(profile.digest(base64.b64decode(row["report_base64"])), row["report_identity"]["sha256"])
            self.assertTrue(Path(row["object"]).is_file())
            self.assertTrue((self.root / "output" / (row["id"] + ".record.json")).is_file())
        for comparison in result["comparisons"]:
            self.assertTrue((comparison["before_job"].startswith("C-") and comparison["after_job"].startswith("I-")) or
                            (comparison["before_job"].startswith("T-") and comparison["after_job"].startswith("U-")))

    def test_missing_malformed_truncated_partial_report_blocks_no_retry(self):
        for failure in ("missing", "malformed", "truncated", "partial", "metric"):
            with self.subTest(failure=failure):
                def behavior(command, kwargs):
                    path = Path(kwargs["env"]["HOTPATH_OUTPUT_PATH"])
                    if failure == "missing":
                        path.unlink()
                    elif failure in ("malformed", "truncated"):
                        path.write_bytes(b'{"type":"hotpath_report"')
                    else:
                        data = json.loads(path.read_text())
                        data["functions_alloc"]["total_count" if failure == "partial" else "profiling_mode"] = 6 if failure == "partial" else "alloc-bytes"
                        self.write(path, data)
                    return 0
                self.behavior = behavior
                self.spawned = []
                output = self.root / ("output-" + failure)
                with patch.object(profile.subprocess, "Popen", side_effect=self.mock_popen):
                    result = profile.execute(self.manifest_path, self.manifest_hash, output, True)
                self.assertEqual(result["status"], "blocked")
                self.assertEqual(len(result["jobs"]), 1)
                self.assertEqual(result["jobs"][0]["process"]["returncode"], 0)
                self.assertTrue((output / "result.json").exists())

    def test_exit_zero_impossible_reports_block_stop_and_retain_complete_raw_evidence(self):
        for kind in ("byte-average", "duration-average", "meta"):
            self.spawned = []
            invalid_raw = []
            def behavior(command, kwargs):
                env = kwargs["env"]
                if kind == "byte-average" and env["HOTPATH_ALLOC_METRIC"] != "bytes":
                    return 0
                original = json.loads(Path(env["HOTPATH_OUTPUT_PATH"]).read_text())
                data = impossible_report(kind, env["HOTPATH_ALLOC_METRIC"], original["label"], "main")
                raw = encoded(data)
                Path(env["HOTPATH_OUTPUT_PATH"]).write_bytes(raw)
                invalid_raw.append(raw)
                return 0
            self.behavior = behavior
            output = self.root / ("output-impossible-" + kind)
            with self.subTest(kind=kind), patch.object(profile.subprocess, "Popen", side_effect=self.mock_popen):
                result = profile.execute(self.manifest_path, self.manifest_hash, output, True)
            self.assertEqual(result["status"], "blocked")
            self.assertEqual(len(result["jobs"]), 2 if kind == "byte-average" else 1)
            failed = result["jobs"][-1]
            self.assertEqual(failed["status"], "blocked")
            self.assertEqual(failed["process"]["returncode"], 0)
            self.assertEqual(len(invalid_raw), 1)
            self.assertEqual(base64.b64decode(failed["report_base64"]), invalid_raw[0])
            self.assertEqual(Path(failed["report"]).read_bytes(), invalid_raw[0])
            self.assertEqual(failed["report_identity"]["sha256"], profile.digest(invalid_raw[0]))
            self.assertEqual(base64.b64decode(failed["process"]["stdout_base64"]), b"synthetic stdout\xff")
            self.assertNotIn("comparisons", result)
            self.assertEqual(sum(command[0] != "/usr/bin/git" for command, kwargs in self.spawned), len(result["jobs"]))
            self.assertEqual(len(json.loads(invalid_raw[0])["functions_alloc"]["data"]), 5)
            self.assertEqual(len(json.loads(invalid_raw[0])["functions_timing"]["data"]), 5)
            retained = json.loads((output / (failed["id"] + ".record.json")).read_text())
            self.assertEqual(retained["report_base64"], failed["report_base64"])

    def test_exit_zero_invalid_meta_types_and_timestamp_block_before_comparison(self):
        for invalid_meta in ({"rustc": "synthetic", "os": [], "created_at": "2000-01-01T00:00:00Z"},
                             {"rustc": "synthetic", "os": "linux-x86_64", "created_at": "2001-02-29T00:00:00Z"}):
            def behavior(command, kwargs):
                path = Path(kwargs["env"]["HOTPATH_OUTPUT_PATH"])
                data = json.loads(path.read_text())
                data["meta"] = invalid_meta
                self.write(path, data)
                return 0
            self.behavior = behavior
            output = self.root / ("output-meta-" + str(len(list(self.root.glob("output-meta-*")))))
            with patch.object(profile.subprocess, "Popen", side_effect=self.mock_popen):
                result = profile.execute(self.manifest_path, self.manifest_hash, output, True)
            self.assertEqual(result["status"], "blocked")
            self.assertEqual(len(result["jobs"]), 1)
            self.assertEqual(result["jobs"][0]["process"]["returncode"], 0)
            raw = base64.b64decode(result["jobs"][0]["report_base64"])
            self.assertEqual(json.loads(raw)["meta"], invalid_meta)

    def test_dirty_wrong_pin_or_staged_source_refuses_before_compiler(self):
        for bad in ("pin", "clean", "staged"):
            original = self.mock_popen
            def mock(command, **kwargs):
                if command[0] == "/usr/bin/git" and ((bad == "pin" and command[3] == "rev-parse") or
                        (bad == "clean" and command[3] == "status") or (bad == "staged" and command[3] == "diff")):
                    kwargs["stdout"].write(b"wrong pin or dirty source\n")
                    child = unittest.mock.Mock(pid=987654321)
                    child.wait.return_value = 0
                    return child
                return original(command, **kwargs)
            with self.subTest(bad=bad), patch.object(profile.subprocess, "Popen", side_effect=mock):
                result = profile.execute(self.manifest_path, self.manifest_hash, self.root / ("output-" + bad), True)
            self.assertEqual(result["status"], "blocked")
            self.assertEqual(result["jobs"], [])
        self.assertFalse(any(command[0] != "/usr/bin/git" for command, kwargs in self.spawned))

    def test_changed_frozen_compiler_refuses_before_any_spawn(self):
        binary = Path(self.manifest["builds"]["main"]["binary"])
        binary.chmod(0o755)
        binary.write_bytes(b"replaced frozen product")
        binary.chmod(0o555)
        with patch.object(profile.subprocess, "Popen") as popen, self.assertRaises(ValueError):
            profile.execute(self.manifest_path, self.manifest_hash, self.root / "output", True)
        popen.assert_not_called()

    def test_writable_frozen_compiler_refused(self):
        Path(self.manifest["builds"]["main"]["binary"]).chmod(0o755)
        with patch.object(profile.subprocess, "Popen") as popen, self.assertRaisesRegex(ValueError, "0555"):
            profile.execute(self.manifest_path, self.manifest_hash, self.root / "output", True)
        popen.assert_not_called()

    def test_preexisting_report_blocks_before_profile_spawn(self):
        original = self.mock_popen
        def mock(command, **kwargs):
            child = original(command, **kwargs)
            if command[0] == "/usr/bin/git" and command[3] == "ls-files":
                (kwargs["cwd"] / "T-corpus-1000-emit-object-count.report.json").write_text("must not truncate")
            return child
        with patch.object(profile.subprocess, "Popen", side_effect=mock):
            result = profile.execute(self.manifest_path, self.manifest_hash, self.root / "output", True)
        self.assertEqual(result["status"], "blocked")
        self.assertEqual(result["jobs"], [])
        self.assertEqual((self.root / "output/T-corpus-1000-emit-object-count.report.json").read_text(), "must not truncate")
        self.assertFalse(any(command[0] != "/usr/bin/git" for command, kwargs in self.spawned))

    def test_created_report_symlink_is_not_accepted(self):
        def behavior(command, kwargs):
            path = Path(kwargs["env"]["HOTPATH_OUTPUT_PATH"])
            copy = self.root / "outside-report"
            path.rename(copy)
            path.symlink_to(copy)
            return 0
        self.behavior = behavior
        result = self.run_mock()
        self.assertEqual(result["status"], "blocked")
        self.assertIn("symlink", result["jobs"][0]["error"])

    def test_post_job_machine_drift_blocks(self):
        original = profile.machine_snapshot
        drift = False
        def snapshot():
            return {"drift": True} if drift else original()
        def behavior(command, kwargs):
            nonlocal drift
            drift = True
            return 0
        self.behavior = behavior
        with patch.object(profile, "machine_snapshot", side_effect=snapshot):
            result = self.run_mock()
        self.assertEqual(result["status"], "blocked")
        self.assertIn("environment drift", result["jobs"][0]["error"])

    def test_nonzero_status_does_not_accept_valid_report(self):
        self.behavior = lambda *args: 1
        result = self.run_mock()
        self.assertEqual(result["status"], "blocked")
        self.assertEqual(len(result["jobs"]), 1)
        self.assertIn("report_base64", result["jobs"][0])

    def test_wrong_object_format_blocks_not_invented_native_execution(self):
        def behavior(command, kwargs):
            Path(command[-1]).write_bytes(b"not an ELF relocatable object")
            return 0
        self.behavior = behavior
        result = self.run_mock()
        self.assertEqual(result["status"], "blocked")
        self.assertIn("ELF64", result["jobs"][0]["error"])

    def test_post_run_source_integrity_blocks(self):
        def behavior(command, kwargs):
            (self.inputs / self.inventory["entries"][0]["source"]).write_bytes(b"modified during child")
            return 0
        self.behavior = behavior
        result = self.run_mock()
        self.assertEqual(result["status"], "blocked")
        self.assertIn("integrity", result["integrity_error"])

    def test_previous_report_replacement_between_jobs_blocks(self):
        first = []
        def behavior(command, kwargs):
            path = Path(kwargs["env"]["HOTPATH_OUTPUT_PATH"])
            if first:
                first[0].write_bytes(b"replaced previous report")
            first.append(path)
            return 0
        self.behavior = behavior
        result = self.run_mock()
        self.assertEqual(result["status"], "blocked")
        self.assertLess(len(result["jobs"]), 16)
        self.assertIn("integrity", result["integrity_error"])

    def test_all_output_paths_guarded_before_spawn(self):
        root = profile.new_root(self.root / "owned")
        for key in ("report", "object", "stdout", "stderr"):
            path = root / ("job." + key)
            path.write_text("do not truncate")
            with self.subTest(key=key), self.assertRaises(ValueError):
                profile.output_guard(root, [path])
            self.assertEqual(path.read_text(), "do not truncate")
            path.unlink()
            path.symlink_to(self.root / "nonexistent")
            with self.subTest(key=key, kind="symlink"), self.assertRaises(ValueError):
                profile.output_guard(root, [path])
            path.unlink()

    def test_existing_root_and_symlinked_parent_refused(self):
        with self.assertRaises(ValueError):
            profile.new_root(self.root)
        (self.root / "alias").symlink_to(self.inputs, target_is_directory=True)
        with self.assertRaises(ValueError):
            profile.new_root(self.root / "alias/output")

    def test_output_escape_refused(self):
        root = profile.new_root(self.root / "owned")
        with self.assertRaises(ValueError):
            profile.output_guard(root, [self.root / "escape"])
        with self.assertRaises(ValueError):
            profile.output_guard(root, [root / "../escape"])

    def test_world_writable_root_or_parent_refused(self):
        root = profile.new_root(self.root / "owned")
        root.chmod(0o777)
        with self.assertRaises(ValueError):
            profile.output_guard(root, [root / "new"])
        with self.assertRaises(ValueError):
            profile.new_root(root / "nested")

    def test_record_identity_replacement_even_same_bytes_refused(self):
        source = self.inputs / self.inventory["entries"][0]["source"]
        before = {str(source): profile.file_identity(source)}
        copy = self.root / "same-bytes"
        copy.write_bytes(source.read_bytes())
        copy.replace(source)
        with self.assertRaises(ValueError):
            profile.check_integrity(before)

    def test_timeout_kills_owned_group_and_retains_raw_bytes(self):
        root = profile.new_root(self.root / "owned")
        def mock(command, **kwargs):
            kwargs["stdout"].write(b"partial\xff")
            kwargs["stderr"].write(b"error")
            child = unittest.mock.Mock(pid=987654321)
            child.wait.side_effect = [subprocess.TimeoutExpired(command, 1), -9]
            return child
        with patch.object(profile.subprocess, "Popen", side_effect=mock) as popen, patch.object(profile.os, "killpg") as kill:
            row = profile.run_child(["synthetic"], profile.BASE_ENV, root, "timeout", 1)
        kill.assert_called_once_with(987654321, signal.SIGKILL)
        self.assertTrue(popen.call_args.kwargs["start_new_session"])
        self.assertEqual(row["status"], "blocked")
        self.assertTrue(row["timeout"])
        self.assertEqual(base64.b64decode(row["stdout_base64"]), b"partial\xff")

    def test_spawn_error_retains_empty_raw_streams(self):
        root = profile.new_root(self.root / "owned")
        with patch.object(profile.subprocess, "Popen", side_effect=OSError("mock refusal")):
            row = profile.run_child(["synthetic"], profile.BASE_ENV, root, "spawn", 1)
        self.assertEqual(row["status"], "blocked")
        self.assertEqual(row["spawn_error"], "mock refusal")
        self.assertEqual(row["stdout_base64"], "")


if __name__ == "__main__":
    unittest.main()
