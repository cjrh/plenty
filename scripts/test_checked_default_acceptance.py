"""Acceptance controller safety/generation tests; no Plenty or Cargo execution."""

import base64
from collections import Counter
import contextlib
import io
import json
from pathlib import Path
import re
import subprocess
import tempfile
import textwrap
import unittest
from unittest.mock import patch

import checked_default_acceptance as acceptance


class CheckedDefaultAcceptanceTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.inputs = self.root / "inputs"

    def generate(self, allocations=False):
        return acceptance.generate(self.inputs, units=(1, 2), allocations=allocations)

    def manifest(self, features=(), purpose="syntax-correctness-only"):
        binary = self.root / "frozen-plenty"
        binary.write_bytes(b"frozen snapshot, not executable")
        build = dict(source_sha="a" * 40, source_tree=str(self.root), binary=str(binary),
                     binary_sha256=acceptance.digest(binary.read_bytes()), frozen_snapshot=True,
                     compiler_profile="debug", cargo_features=list(features), cargo_flags=["--locked"],
                     rustflags="", runtime_rustflags="", target="x86_64-unknown-linux-gnu",
                     toolchain="rustc example", runtime_archive_sha256="b" * 64,
                     build_command=["cargo", "build", "--locked"], native_linker="cc example")
        data = dict(schema=1, purpose=purpose, builds={"main": dict(build), "candidate": dict(build)})
        path = self.root / "build.json"
        acceptance.write_json(path, data)
        return path, data

    def load_manifest(self, path, measurement=False):
        with patch.object(acceptance.subprocess, "check_output", return_value="a" * 40 + "\n"), \
             patch.object(acceptance.subprocess, "run"):
            return acceptance.load_manifest(path, measurement)

    def validate(self, manifest, **kwargs):
        with patch.object(acceptance, "load_manifest", return_value=manifest):
            return acceptance.validate(self.inputs, self.root / "unused.json", self.root / "validation", **kwargs)

    def test_generation_never_executes_commands(self):
        with patch.object(acceptance.subprocess, "run", side_effect=AssertionError("executed")), \
             patch.object(acceptance.subprocess, "check_output", side_effect=AssertionError("executed")):
            inventory = self.generate()
        self.assertEqual(inventory["purpose"], "unvalidated-inputs-not-acceptance")
        self.assertEqual(len(inventory["entries"]), 58)
        self.assertTrue(inventory["unimplemented"])
        self.assertEqual(acceptance.load_inventory(self.inputs), inventory)

    def test_existing_output_is_not_replaced_even_if_empty(self):
        self.inputs.mkdir()
        with self.assertRaises(FileExistsError):
            self.generate()
        self.assertEqual(list(self.inputs.iterdir()), [])

    def test_sources_and_inputs_are_deterministic(self):
        first = self.generate()
        other = self.root / "other"
        second = acceptance.generate(other, units=(1, 2))
        self.assertEqual(first, second)
        for entry in first["entries"]:
            self.assertEqual((self.inputs / entry["source"]).read_bytes(), (other / entry["source"]).read_bytes())

    def test_arithmetic_helpers_have_independent_modes(self):
        for mode in acceptance.MODES:
            calls = acceptance.calls_source(mode)
            self.assertEqual(calls.count("value * factor + bias"), 1)
            self.assertEqual(calls.count("base + i"), 1)
            if mode in ("C", "U"):
                keyword = "checked:" if mode == "C" else "unchecked:"
                self.assertEqual(calls.count(keyword), 2)
            if mode in ("C", "I"):
                self.assertIn("total += leaf(base + i, factor, bias)?", calls)
                self.assertIn("Result[i64, OverflowError[i64]]", calls)
            else:
                self.assertIn("def leaf(value: i64, factor: i64, bias: i64) -> i64:", calls)
            if mode == "I":
                self.assertIn("Ok(value * factor + bias)", calls)
                self.assertIn("Ok(total)", calls)
            if mode == "C":
                self.assertIn("checked:\n        value * factor + bias", calls)

    def test_parsing_and_setup_precede_kernel_calls(self):
        source = acceptance.tight_source("I")
        main = source[source.index("def main"):]
        self.assertLess(main.index("i64.parse"), main.index("report(kernel"))
        text = acceptance.text_source("I")
        main = text[text.index("def main"):]
        self.assertLess(main.index(".repeat(4096)?"), main.index("match consume"))
        self.assertNotIn("monotonic", text)  # This stage never claims an internal interval.

    def test_early_late_and_first_operation_error_oracles(self):
        inventory = self.generate()
        entries = {entry["source"]: entry for entry in inventory["entries"]}
        tight = entries["sources/C/tight.plenty"]["cases"]
        self.assertEqual(tight[-1]["args"], ["1000000", str(acceptance.MAX - 999999), "1"])
        self.assertIn(str(acceptance.MIN), tight[-1]["stdout"])
        calls = entries["sources/I/calls.plenty"]["cases"]
        self.assertEqual(calls[1]["stdout"], "overflow -2\nafter\n")
        self.assertEqual(calls[2]["stdout"], f"overflow {acceptance.MIN}\nafter\n")
        self.assertEqual(entries["sources/T/calls.plenty"]["cases"][1]["expected"], "arithmetic-trap")
        self.assertNotIn("sources/U/recovered-errors.plenty", entries)
        self.assertNotIn("sources/T/recovered-errors.plenty", entries)
        recovery = acceptance.recovery_source("I")
        self.assertIn("errors.wrapping_add(1)", recovery)
        self.assertIn("checksum.wrapping_add(error.wrapped)", recovery)

    def test_all_eight_widths_use_full_boundary_inputs(self):
        entries = self.generate()["entries"]
        for mode in acceptance.MODES:
            widths = [entry for entry in entries if entry["mode"] == mode and "/add-" in entry["source"]]
            self.assertEqual(len(widths), 8)
            for entry in widths:
                ty = Path(entry["source"]).stem.removeprefix("add-")
                bits, signed = int(ty[1:]), ty[0] == "i"
                maximum = (1 << (bits - int(signed))) - 1
                self.assertEqual(entry["cases"][-1]["args"], [str(maximum), "1"])
                self.assertIn(f"{ty}.parse", (self.inputs / entry["source"]).read_text())
                if mode in ("C", "I"):
                    self.assertIn(str(acceptance.wrap(maximum + 1, bits, signed)), entry["cases"][-1]["stdout"])

    def test_text_output_and_budget_oracles_are_independent(self):
        self.assertEqual(len(acceptance.TEXT), 94208)
        self.assertEqual(len(acceptance.TEXT_OUTPUT), 94207)
        self.assertEqual(textwrap.fill(acceptance.TEXT, width=10), acceptance.TEXT_OUTPUT)
        self.assertEqual(acceptance.digest(acceptance.TEXT_OUTPUT.encode()),
                         "935903d0be29d953df6258c79693ec52064241fc2d8848f04bce860ce014da68")
        text = [entry for entry in self.generate()["entries"] if entry["source"] == "sources/T/text.plenty"][0]
        self.assertEqual(text["cases"][1]["stdout"], "ok 9420700\nafter\n")
        self.assertEqual(text["cases"][2]["args"], ["100", "8388608", "94207"])
        self.assertEqual(text["cases"][3]["args"], ["100", "94206", "8388608"])
        self.assertEqual(text["cases"][3]["expected"], "handled-error")

    def test_cleanup_owners_are_inside_equivalent_boundaries(self):
        checked = acceptance.cleanup_source("C")
        implicit = acceptance.cleanup_source("I")
        self.assertIn("checked:\n        owner = Owner(10)", checked)
        self.assertIn("def cleanup", implicit)
        self.assertIn("    owner = Owner(10)", implicit)
        self.assertIn("*n += i64.MAX", implicit)
        self.assertIn('print("after", n)?', implicit)

    def test_marker_probes_are_opt_in_and_have_allocating_control(self):
        self.assertFalse(any(e["requires_runtime_checks"] for e in self.generate()["entries"]))
        other = acceptance.generate(self.root / "markers", units=(1,), allocations=True)
        marked = [e for e in other["entries"] if e["requires_runtime_checks"]]
        self.assertEqual(len(marked), 4)
        source = acceptance.allocation_source("I")
        begin, end = source.index('print("__test_begin'), source.index('print("__test_end')
        self.assertNotIn("print(", source[begin + len('print("__test_begin_no_allocations__").unwrap()'):end])
        self.assertIn("overflow = step(i64.MAX, 0)", source)
        control = acceptance.allocation_source("I", True)
        self.assertLess(control.index("begin_no_allocations"), control.index("values = [1, 2, 3]"))
        self.assertLess(control.index("values = [1, 2, 3]"), control.index("end_no_allocations"))

    def test_original_template_extracted_without_workload_edits(self):
        rust, template = acceptance.original_template(acceptance.REPO)
        self.assertIn(template, rust)
        original = acceptance.corpus_source("T", 2, template)
        self.assertEqual(original, template.replace("{i}", "0") + template.replace("{i}", "1")
                         + "def main() -> Result[(), Failure]:\n    print(work1(5)?)?\n    Ok(())\n")
        self.assertEqual(acceptance.generate(self.inputs, units=(1000, 4000))["units"], [1000, 4000])

    def test_four_corpus_inventories_preserve_arithmetic_and_calls(self):
        _, template = acceptance.original_template(acceptance.REPO)
        sources = {mode: acceptance.corpus_source(mode, 2, template) for mode in acceptance.MODES}
        for source in sources.values():
            self.assertEqual(len(re.findall(r"^protocol ", source, re.M)), 2)
            self.assertEqual(len(re.findall(r"^class ", source, re.M)), 2)
            self.assertEqual(len(re.findall(r"^enum ", source, re.M)), 2)
            self.assertEqual(len(re.findall(r"^\s*def ", source, re.M)), 13)
            for operation in ("self.x * self.x + self.y * self.y +", "self.x + amount", "self.y + amount",
                              "value +", "[n * n +", "if n % 2 == 0", "total = total + v", "point.shift(1)",
                              "point.squared_length()", "for v in &values:", "range(limit)"):
                self.assertEqual(source.count(operation), 2, operation)
            self.assertIn("print(work1(5)?)?", source)
        self.assertIn("Result[list[i64], AllocError | OverflowError[i64] | ZeroDivisionError]", sources["I"])
        self.assertIn("point.shift(1)?", sources["C"])
        self.assertNotIn("wrapping_", sources["U"])
        for count, checksum in ((1000, 1004057), (4000, 16016057)):
            i = count - 1
            self.assertEqual(36 + (i + 1) ** 2 + i + 5 + i + (4 + i) + (16 + i), checksum)

    def test_template_drift_is_reported_not_silently_regenerated(self):
        (self.root / "examples").mkdir()
        (self.root / "examples/compile_bench.rs").write_text("changed template")
        with self.assertRaisesRegex(ValueError, "template changed"):
            acceptance.generate(self.inputs, repo=self.root)
        self.assertFalse(self.inputs.exists())

    def test_order_is_reproducible_balanced_and_keeps_all_rounds(self):
        rows = acceptance.schedule()
        self.assertEqual(rows, acceptance.schedule())
        for phase, count in (("warmup", 5), ("sample", 30)):
            chosen = [row for row in rows if row["phase"] == phase]
            self.assertEqual(Counter(row["mode"] for row in chosen), dict.fromkeys(acceptance.MODES, count))
            for start in range(0, count - 3, 4):
                for position in range(4):
                    self.assertEqual(set(row["mode"] for row in chosen[start * 4 + position:(start + 4) * 4:4]), set(acceptance.MODES))
        self.assertEqual(len(acceptance.schedule(3, 15)), 72)

    def test_manifest_pins_hashes_and_frozen_requirement(self):
        path, data = self.manifest()
        self.assertEqual(self.load_manifest(path), data)
        for key, value in (("source_sha", "short"), ("binary_sha256", "0" * 64),
                           ("frozen_snapshot", False), ("binary", "relative"), ("build_command", [])):
            with self.subTest(key=key):
                changed = json.loads(json.dumps(data))
                changed["builds"]["candidate"][key] = value
                acceptance.write_json(path, changed)
                with self.assertRaises(ValueError):
                    self.load_manifest(path)
        acceptance.write_json(path, data)
        with patch.object(acceptance.subprocess, "check_output", return_value="c" * 40 + "\n"):
            with self.assertRaisesRegex(ValueError, "source pin mismatch"):
                acceptance.load_manifest(path)

    def test_mismatched_debug_builds_allowed_only_for_correctness(self):
        path, data = self.manifest(purpose="measurement")
        data["quiet_machine_approved"] = True
        data["builds"]["candidate"]["cargo_features"] = ["runtime-checks"]
        acceptance.write_json(path, data)
        self.load_manifest(path)
        with self.assertRaisesRegex(ValueError, "settings do not match"):
            self.load_manifest(path, True)
        data["builds"]["main"]["cargo_features"] = ["runtime-checks"]
        acceptance.write_json(path, data)
        self.load_manifest(path, True)
        data["quiet_machine_approved"] = False
        acceptance.write_json(path, data)
        with self.assertRaisesRegex(ValueError, "quiet-machine"):
            self.load_manifest(path, True)

    def test_inventory_refuses_modified_duplicate_and_escaping_sources(self):
        inventory = self.generate()
        path = self.inputs / "inventory.json"
        entry = inventory["entries"][0]
        source = self.inputs / entry["source"]
        original = source.read_bytes()
        source.write_bytes(original + b"changed")
        with self.assertRaisesRegex(ValueError, "changed source"):
            acceptance.load_inventory(self.inputs)
        source.write_bytes(original)
        inventory["entries"].append(dict(entry))
        acceptance.write_json(path, inventory)
        with self.assertRaisesRegex(ValueError, "duplicate"):
            acceptance.load_inventory(self.inputs)
        inventory["entries"].pop()
        entry["source"] = "../outside.plenty"
        acceptance.write_json(path, inventory)
        with self.assertRaisesRegex(ValueError, "escapes"):
            acceptance.load_inventory(self.inputs)

    def test_classification_keeps_traps_errors_signals_panics_and_timeouts_distinct(self):
        expected = acceptance.case("trap", (), trap=True)
        row = dict(returncode=1, stdout="", stderr="error: tight.plenty:3: integer overflow\n")
        self.assertEqual(acceptance.classify(row, expected, "tight.plenty"), "expected-arithmetic-trap")
        for change, outcome in ((dict(returncode=-4), "unexpected-signal"), (dict(timeout=True), "timeout"),
                                (dict(stderr="panicked: integer overflow"), "unexpected-panic"),
                                (dict(stderr="assertion failed"), "unexpected-panic"),
                                (dict(stdout="after\n"), "wrong-outcome"),
                                (dict(stderr="integer overflow"), "wrong-outcome"),
                                (dict(stderr="error: other.plenty:3: integer overflow\n"), "wrong-outcome")):
            self.assertEqual(acceptance.classify(dict(row, **change), expected, "tight.plenty"), outcome)
        error = acceptance.case("recovered", (), "overflow -2\n", classification="handled-error")
        self.assertEqual(acceptance.classify(dict(returncode=0, stdout="overflow -2\n", stderr=""), error, "x"), "expected-handled-error")
        self.assertEqual(acceptance.classify(dict(returncode=1, stdout="overflow -2\n", stderr=""), error, "x"), "wrong-outcome")

    def test_execute_preserves_non_utf8_and_timeout_streams_without_timing(self):
        with patch.object(acceptance.subprocess, "run", return_value=subprocess.CompletedProcess(["probe"], 0, b"\xff", b"err")) as run:
            row = acceptance.execute(["probe"], {}, 3)
            self.assertEqual(base64.b64decode(row["stdout_base64"]), b"\xff")
            self.assertNotIn("seconds", row)
            run.assert_called_once()
        with patch.object(acceptance.subprocess, "run", side_effect=subprocess.TimeoutExpired(["probe"], 3, b"partial", b"failure")):
            row = acceptance.execute(["probe"], {}, 3)
            self.assertTrue(row["timeout"])
            self.assertEqual(row["stdout"], "partial")
            self.assertIsNone(row["returncode"])

    def test_validate_dry_run_records_both_profiles_no_execution(self):
        self.generate()
        _, manifest = self.manifest()
        with patch.object(acceptance, "execute", side_effect=AssertionError("executed")):
            self.validate(manifest, dry_run=True, families=["calls"])
        rows = [json.loads(line) for line in (self.root / "validation/results.jsonl").read_text().splitlines()]
        self.assertEqual(set(row["profile"] for row in rows), {"debug", "release"})
        self.assertEqual(set(row["classification"] for row in rows), {"not-run"})
        self.assertEqual(len(rows), 48)
        self.assertFalse(json.loads((self.root / "validation/result.json").read_text())["performance_evidence"])
        self.assertFalse(any("--release" in row["command"] and "--emit-object" in row["command"] for row in rows))

    def test_failed_build_never_runs_or_retries_and_retains_every_failure(self):
        self.generate()
        _, manifest = self.manifest()
        def run(command, env, timeout):
            self.assertEqual(env["PLENTY_CACHE"], "off")
            return dict(command=command, returncode=0 if "--check" in command else 1,
                        timeout=False, stdout="", stderr="diagnostic")
        with patch.object(acceptance, "execute", side_effect=run) as execute:
            self.assertFalse(self.validate(manifest, families=["tight"]))
        self.assertEqual(execute.call_count, 16)  # check + failed build, four modes, two profiles
        rows = [json.loads(line) for line in (self.root / "validation/results.jsonl").read_text().splitlines()]
        self.assertEqual(sum(row["classification"] == "build-failed" for row in rows), 8)
        self.assertFalse(any(row["phase"] == "runtime" for row in rows))
        self.assertFalse(any("seconds" in row for row in rows))

    def test_allocation_probes_refuse_noninstrumented_manifest_before_output(self):
        self.generate(allocations=True)
        _, manifest = self.manifest()
        with self.assertRaisesRegex(ValueError, "runtime-checks"):
            self.validate(manifest, dry_run=True, families=["noalloc"])
        self.assertFalse((self.root / "validation").exists())

    def test_validation_refuses_output_reuse(self):
        self.generate()
        _, manifest = self.manifest()
        self.validate(manifest, dry_run=True, families=["tight"])
        original = (self.root / "validation/results.jsonl").read_bytes()
        with self.assertRaises(FileExistsError):
            self.validate(manifest, dry_run=True, families=["tight"])
        self.assertEqual((self.root / "validation/results.jsonl").read_bytes(), original)

    def test_invalid_unit_counts_do_not_create_outputs(self):
        for units in ((), (0,), (-1,), (True,), (1, 1)):
            with self.subTest(units=units), self.assertRaisesRegex(ValueError, "positive integers"):
                acceptance.generate(self.inputs, units=units)
        self.assertFalse(self.inputs.exists())

    def test_unknown_family_and_invalid_timeout_refuse_before_output(self):
        self.generate()
        _, manifest = self.manifest()
        for kwargs in (dict(families=["tight", "typo"]), dict(timeout=0)):
            with self.assertRaises(ValueError):
                self.validate(manifest, dry_run=True, **kwargs)
        self.assertFalse((self.root / "validation").exists())

    def test_dirty_source_manifest_is_rejected(self):
        path, _ = self.manifest()
        with patch.object(acceptance.subprocess, "check_output", return_value="a" * 40 + "\n"), \
             patch.object(acceptance.subprocess, "run", side_effect=subprocess.CalledProcessError(1, ["git", "diff"])):
            with self.assertRaises(subprocess.CalledProcessError):
                acceptance.load_manifest(path)

    def test_spawn_errors_are_retained_not_retried_or_classified_as_traps(self):
        with patch.object(acceptance.subprocess, "run", side_effect=PermissionError("denied")) as run:
            row = acceptance.execute(["probe"], {}, 3)
        run.assert_called_once()
        self.assertEqual(row["spawn_error"], "denied")
        self.assertEqual(acceptance.classify(row, acceptance.case("trap", (), trap=True), "probe"), "spawn-error")
        self.assertEqual(base64.b64decode(row["stderr_base64"]), b"denied")

    def test_all_runtime_results_are_retained_without_profile_or_outlier_filtering(self):
        inventory = self.generate()
        _, manifest = self.manifest()
        cases = next(entry["cases"] for entry in inventory["entries"] if entry["source"] == "sources/I/tight.plenty")
        invocations = []
        def run(command, env, timeout):
            self.assertNotIn("PLENTY_PROFILE", env)
            self.assertNotIn("HOTPATH_OUTPUT_PATH", env)
            if "--check" in command or "--compile" in command:
                return dict(command=command, returncode=0, timeout=False, stdout="", stderr="")
            mode = Path(command[0]).parts[-3]
            expected = next(item for item in cases if item["args"] == command[1:])
            invocations.append((mode, expected["name"]))
            if expected["name"] in ("early", "late") and mode in ("T", "U"):
                return dict(command=command, returncode=1, timeout=False, stdout="", stderr="error: tight.plenty:4: integer overflow\n")
            return dict(command=command, returncode=0, timeout=False, stdout=expected["stdout"], stderr="")
        with patch.dict(acceptance.os.environ, {"PLENTY_PROFILE": "1", "HOTPATH_OUTPUT_PATH": "/unused"}), \
             patch.object(acceptance, "execute", side_effect=run):
            self.assertTrue(self.validate(manifest, families=["tight"]))
        self.assertEqual(len(invocations), 40)
        rows = [json.loads(line) for line in (self.root / "validation/results.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 56)
        self.assertEqual(sum(row["classification"] == "expected-arithmetic-trap" for row in rows), 8)
        self.assertEqual(sum(row["classification"] == "expected-handled-error" for row in rows), 8)
        self.assertFalse(any("seconds" in row for row in rows))

    def test_measure_cannot_execute_even_with_permission_and_valid_manifest(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as exit:
            acceptance.main(["measure", "--build-manifest", "/missing"])
        self.assertEqual(exit.exception.code, 2)
        path, _ = self.manifest(purpose="measurement")
        with patch.object(acceptance, "load_manifest") as load, contextlib.redirect_stderr(io.StringIO()), \
             self.assertRaises(SystemExit) as exit:
            acceptance.main(["measure", "--allow-measurements", "--build-manifest", str(path)])
        self.assertEqual(exit.exception.code, 2)
        load.assert_called_once_with(path, measurement=True)


if __name__ == "__main__":
    unittest.main()
