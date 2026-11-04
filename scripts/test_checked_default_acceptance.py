"""Acceptance controller safety/generation tests; no Plenty or Cargo execution."""

import base64
from collections import Counter
import contextlib
import io
import json
from pathlib import Path
import struct
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

    def reject_inventory_before_output(self, inventory, message):
        acceptance.write_json(self.inputs / "inventory.json", inventory)
        with patch.object(acceptance, "load_manifest", side_effect=AssertionError("manifest must not load")), \
             self.assertRaisesRegex(ValueError, message):
            acceptance.validate(self.inputs, self.root / "unused.json", self.root / "validation",
                                dry_run=True, modes=["I"])
        self.assertFalse((self.root / "validation").exists())

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
            self.assertEqual(calls.count("product = value * factor"), 1)
            self.assertEqual(calls.count("product + bias"), 1)
            self.assertLess(calls.index("product = value * factor"), calls.index("product + bias"))
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
                self.assertIn("Ok(product + bias)", calls)
                self.assertIn("Ok(total)", calls)
            if mode == "C":
                self.assertIn("checked:\n        product = value * factor\n        product + bias", calls)

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

    def test_cleanup_adds_no_rollback_without_changing_original_cases_or_helper(self):
        inventory = self.generate()
        for mode in acceptance.MODES:
            source = acceptance.cleanup_source(mode)
            original = acceptance.function(mode, "cleanup", "n: &mut i64", "()",
                "AllocError | OverflowError[i64]", "owner = Owner(10)\nitems = [1, 2, 3]?\n*n += i64.MAX\ndrop(owner)\ndrop(items)\n",
                "()", public=True)
            self.assertIn(original, source)
            additional = source[source.index("def cleanup_after_mutation"):source.index("def main")]
            for before, after in (("owner = Owner(10)", "items = [1, 2, 3]?"),
                                  ("items = [1, 2, 3]?", "*n += 1"),
                                  ("*n += 1", "*n += i64.MAX"), ("*n += i64.MAX", "drop(owner)")):
                self.assertLess(additional.index(before), additional.index(after))
            self.assertIn("-> Result[(), AllocError | OverflowError[i64]]:", additional)
            self.assertTrue(additional.rstrip().endswith("()" if mode == "C" else "Ok(())"))  # No nonunit sentinel for #712.
            entry = next(e for e in inventory["entries"] if e["source"] == f"sources/{mode}/cleanup.plenty")
            self.assertEqual([c["name"] for c in entry["cases"]], ["success", "skipped-write", "earlier-mutation-retained"])
            self.assertEqual(entry["cases"][0]["stdout"], f"drop 10\nok\nafter {acceptance.MAX}\n")
            expected = entry["cases"][2]
            self.assertEqual(expected["args"], ["0", "no-rollback"])
            if mode in ("C", "I"):
                self.assertEqual(entry["cases"][1]["stdout"], f"drop 10\noverflow {acceptance.MIN}\nafter 1\n")
                self.assertEqual(expected["stdout"], entry["cases"][1]["stdout"])
            else:
                self.assertEqual(entry["cases"][1]["expected"], "arithmetic-trap")
                self.assertEqual(expected["expected"], "arithmetic-trap")

    def test_affine_helper_uses_live_product_for_calls_and_recovery(self):
        for mode in acceptance.MODES:
            leaf = acceptance.affine_leaf(mode)
            self.assertEqual(leaf.count("value * factor"), 1)
            self.assertEqual(leaf.count("product + bias"), 1)
            self.assertNotIn("wrapping_", leaf)
            self.assertIn(leaf, acceptance.calls_source(mode))
            if mode in ("C", "I"):
                self.assertIn(leaf, acceptance.recovery_source(mode))

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

    def test_corpus_work_errors_are_precise_without_other_workload_changes(self):
        _, template = acceptance.original_template(acceptance.REPO)
        original_hashes = {
            "T": "db92390612e4c33af4b98fed9c556f3f088ed198ee013599bcfbab2f8ecc678e",
            "C": "ed1abe01b9f068828b76fc5f81787dc4145a6825d5e5eb48906aba6290f30966",
            "I": "d4acd9811498902a6a8671c694752e715c1e71dcec01eaa0452737be0d36172c",
            "U": "3947eef4729e06f9bce845040c3e48a2ddeacb423b07062639bb919e92441573",
        }
        precise = "AllocError | OverflowError[i64] | ZeroDivisionError"
        for mode in acceptance.MODES:
            source = acceptance.corpus_source(mode, 2, template)
            normalized = source
            for i in range(2):
                declared = precise if mode in ("C", "I") else "Failure"
                declaration = f"def work{i}(a: i64) -> Result[i64, {declared}]:"
                self.assertIn(declaration, source)
                normalized = normalized.replace(declaration, f"def work{i}(a: i64) -> Result[i64, Failure]:")
            self.assertIn("def main() -> Result[(), Failure]:\n    print(work1(5)?)?", source)
            # Only C/I work declarations differ from reviewed commit 628f71aa.
            self.assertEqual(acceptance.digest(normalized.encode()), original_hashes[mode])

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

    def test_inventory_rejects_internal_dot_and_parent_aliases_before_output(self):
        inventory = self.generate()
        original = inventory["entries"][0]
        for alias in (original["source"], "sources/T/./tight.plenty", "sources/T/../T/tight.plenty"):
            with self.subTest(alias=alias):
                inventory["entries"].append(dict(original, source=alias))
                self.reject_inventory_before_output(inventory, "duplicate resolved source")
                inventory["entries"].pop()

    def test_inventory_rejects_internal_file_symlink_alias_across_modes_before_output(self):
        inventory = self.generate()
        original = inventory["entries"][0]
        alias = self.inputs / "sources/T/other-name.plenty"
        alias.symlink_to(self.inputs / original["source"])
        inventory["entries"].append(dict(original, source=str(alias.relative_to(self.inputs)), mode="C"))
        self.reject_inventory_before_output(inventory, "duplicate resolved source")

    def test_inventory_rejects_internal_directory_symlink_alias_before_output(self):
        inventory = self.generate()
        original = inventory["entries"][0]
        (self.inputs / "sources/alias").symlink_to(self.inputs / "sources/T", target_is_directory=True)
        inventory["entries"].append(dict(original, source="sources/alias/tight.plenty"))
        self.reject_inventory_before_output(inventory, "duplicate resolved source")

    def test_inventory_rejects_distinct_same_mode_stem_outputs_before_output(self):
        inventory = self.generate()
        original = inventory["entries"][0]
        other = self.inputs / "sources/other/tight.plenty"
        other.parent.mkdir()
        other.write_bytes((self.inputs / original["source"]).read_bytes())
        inventory["entries"].append(dict(original, source=str(other.relative_to(self.inputs))))
        self.reject_inventory_before_output(inventory, "duplicate executable output identity")

    def test_inventory_preserves_symlink_containment_guard_before_output(self):
        inventory = self.generate()
        original = inventory["entries"][0]
        outside = self.root / "outside.plenty"
        outside.write_bytes((self.inputs / original["source"]).read_bytes())
        alias = self.inputs / "sources/T/outside-alias.plenty"
        alias.symlink_to(outside)
        inventory["entries"].append(dict(original, source=str(alias.relative_to(self.inputs))))
        self.reject_inventory_before_output(inventory, "escapes")

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
        self.assertEqual(len(rows), 52)
        self.assertEqual(sum(row["phase"] == "retention" for row in rows), 4)
        self.assertFalse(json.loads((self.root / "validation/result.json").read_text())["performance_evidence"])
        self.assertFalse(any("--release" in row["command"] and "--emit-object" in row["command"] for row in rows))

    def test_retention_parser_requires_actual_exact_direct_call_in_caller(self):
        caller = "__plenty_fn_calls"
        valid = "00000000001000 <__plenty_fn_calls>:\n  1000:\tcall   1100 <__plenty_fn_leaf>\n"
        self.assertEqual(len(acceptance.retained_calls(valid, caller)), 1)
        self.assertEqual(len(acceptance.retained_calls(valid.replace("call   ", "callq  ", 1), caller)), 1)
        self.assertEqual(len(acceptance.retained_calls(valid.replace("call   ", "bl     "), caller)), 1)
        for wrong in (valid.replace("call   ", "jmp    "), valid.replace("<__plenty_fn_leaf>", "<__plenty_fn_leaf+0x1>"),
                      valid.replace("<__plenty_fn_calls>:", "<other>:"), valid.replace("call   ", "# call "),
                      "00000000001100 <__plenty_fn_leaf>:\n  1100: ret\n", "symbol=__plenty_fn_leaf; call fn0"):
            with self.subTest(wrong=wrong):
                self.assertEqual(acceptance.retained_calls(wrong, caller), [])
        self.assertEqual(acceptance.retained_calls(valid + "00000000001200 <other>:\n  1200: call 1100 <__plenty_fn_leaf>\n", caller),
                         acceptance.retained_calls(valid, caller))

    def test_native_retention_gate_blocks_unqualified_runtime_and_preserves_raw_evidence(self):
        self.generate()
        _, manifest = self.manifest()
        def run(command, env, timeout):
            if command[0] == "/owned/objdump":
                return dict(command=command, returncode=0, timeout=False,
                            stdout="00000000001000 <__plenty_fn_calls>:\n  1000: imul %rsi,%rax\n", stderr="")
            self.assertTrue("--check" in command or "--compile" in command)
            return dict(command=command, returncode=0 if "--release" in command else 1,
                        timeout=False, stdout="", stderr="check failed" if "--check" in command else "")
        # Force only debug builds to fail; checks must succeed independently.
        def command_result(command, env, timeout):
            if "--check" in command:
                return dict(command=command, returncode=0, timeout=False, stdout="", stderr="")
            return run(command, env, timeout)
        with patch.object(acceptance, "execute", side_effect=command_result):
            self.assertFalse(self.validate(manifest, families=["calls"], objdump="/owned/objdump"))
        rows = [json.loads(line) for line in (self.root / "validation/results.jsonl").read_text().splitlines()]
        gates = [row for row in rows if row["phase"] == "retention"]
        self.assertEqual(len(gates), 4)
        self.assertTrue(all(row["classification"] == "retention-blocked" for row in gates))
        self.assertTrue(all(row["static_call_sites"] == [] for row in gates))
        self.assertTrue(all("imul" in row["stdout"] for row in gates))
        self.assertFalse(any(row["phase"] == "runtime" for row in rows))

    def test_native_retention_gate_passes_only_with_one_call_and_successful_tool(self):
        self.generate()
        _, manifest = self.manifest()
        for family in ("calls", "recovered-errors"):
            target = self.root / ("validation-" + family)
            def run(command, env, timeout):
                if "--compile" in command:
                    Path(command[-1]).write_bytes(b"native bytes")
                if command[0] == "objdump":
                    caller = command[-2].split("=", 1)[1]
                    stdout = f"00000000001000 <{caller}>:\n  1000: call 1100 <__plenty_fn_leaf>\n"
                else:
                    self.assertTrue("--check" in command or "--compile" in command)
                    stdout = ""
                return dict(command=command, returncode=0, timeout=False, stdout=stdout, stderr="")
            # No native program executes in this mock; omit cases to isolate the gate.
            inventory = acceptance.load_inventory(self.inputs)
            for entry in inventory["entries"]:
                entry["cases"] = []
            acceptance.write_json(self.inputs / "inventory.json", inventory)
            with patch.object(acceptance, "execute", side_effect=run), \
                 patch.object(acceptance, "load_manifest", return_value=manifest):
                self.assertTrue(acceptance.validate(self.inputs, self.root / "unused", target, families=[family]))
            gates = [json.loads(line) for line in (target / "results.jsonl").read_text().splitlines()
                     if json.loads(line)["phase"] == "retention"]
            self.assertEqual(len(gates), 4 if family == "calls" else 2)
            self.assertTrue(all(row["classification"] == "retained-call" for row in gates))
            self.assertTrue(all(row["executable_sha256"] == acceptance.digest(b"native bytes") for row in gates))

    def test_retention_gate_rejects_failed_timed_out_missing_and_multiple_call_extractions(self):
        inventory = self.generate()
        for entry in inventory["entries"]:
            entry["cases"] = []  # Isolate extraction, not a native semantic proof.
        acceptance.write_json(self.inputs / "inventory.json", inventory)
        _, manifest = self.manifest()
        assembly = "00000000001000 <__plenty_fn_calls>:\n  1000: call 1100 <__plenty_fn_leaf>\n"
        failures = [(1, False, None, assembly), (0, True, None, assembly),
                    (None, False, "missing objdump", ""),
                    (0, False, None, assembly + "  1005: call 1100 <__plenty_fn_leaf>\n")]
        for index, (code, timeout, spawn, stdout) in enumerate(failures):
            output = self.root / f"blocked-{index}"
            def run(command, env, limit):
                if command[0] == "objdump":
                    return dict(command=command, returncode=code, timeout=timeout, spawn_error=spawn,
                                stdout=stdout, stderr="inspection failure" if code else "")
                if "--compile" in command:
                    Path(command[-1]).write_bytes(b"native bytes")
                return dict(command=command, returncode=0, timeout=False, stdout="", stderr="")
            with self.subTest(index=index), patch.object(acceptance, "execute", side_effect=run), \
                 patch.object(acceptance, "load_manifest", return_value=manifest):
                self.assertFalse(acceptance.validate(self.inputs, self.root / "unused", output,
                                                    families=["calls"], modes=["I"]))
            rows = [json.loads(line) for line in (output / "results.jsonl").read_text().splitlines()]
            gates = [row for row in rows if row["phase"] == "retention"]
            self.assertEqual(len(gates), 1)
            self.assertEqual(gates[0]["classification"], "retention-blocked")
            self.assertEqual(gates[0]["stdout"], stdout)
            self.assertFalse(json.loads((output / "result.json").read_text())["passed"])

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

    def test_mode_filter_records_candidate_only_allocation_submatrix_without_changing_flags(self):
        self.generate(allocations=True)
        _, manifest = self.manifest()
        manifest["builds"]["candidate"]["cargo_features"] = ["runtime-checks"]
        with patch.object(acceptance, "execute", side_effect=AssertionError("dry-run must not execute")):
            self.validate(manifest, dry_run=True, families=["noalloc", "allocation-control"], modes=["I"])
        root = self.root / "validation"
        rows = [json.loads(line) for line in (root / "results.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 12)
        self.assertEqual({row["mode"] for row in rows}, {"I"})
        self.assertTrue(all(row["classification"] == "not-run" for row in rows))
        metadata = json.loads((root / "validation.json").read_text())
        self.assertEqual(metadata["selected_modes"], ["I"])
        self.assertEqual(metadata["selected_sources"], ["sources/I/noalloc.plenty", "sources/I/allocation-control.plenty"])
        self.assertEqual(json.loads((root / "build-manifest.json").read_text()), manifest)
        self.assertEqual(manifest["builds"]["main"]["cargo_features"], [])

    def test_default_allocation_submatrix_still_refuses_uninstrumented_baseline(self):
        self.generate(allocations=True)
        _, manifest = self.manifest()
        manifest["builds"]["candidate"]["cargo_features"] = ["runtime-checks"]
        with self.assertRaisesRegex(ValueError, "runtime-checks"):
            self.validate(manifest, dry_run=True, families=["noalloc", "allocation-control"])
        self.assertFalse((self.root / "validation").exists())

    def test_mode_filter_rejects_invalid_or_empty_selections_before_output(self):
        self.generate()
        _, manifest = self.manifest()
        for modes in ([], ["unknown"], ["I", "unknown"]):
            with self.subTest(modes=modes), self.assertRaisesRegex(ValueError, "selected modes"):
                self.validate(manifest, dry_run=True, modes=modes)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as exit:
            acceptance.main(["validate", "--inputs", str(self.inputs), "--build-manifest", "/unused",
                             "--output", str(self.root / "validation"), "--mode", "unknown"])
        self.assertEqual(exit.exception.code, 2)
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

    def test_measure_requires_explicit_new_permission_before_any_inputs_or_commands(self):
        with self.assertRaisesRegex(ValueError, "allow-perf"), \
             patch.object(acceptance, "process_group", side_effect=AssertionError("native command")):
            acceptance.measure("/missing", "a" * 64, "/missing", self.root / "out")
        self.assertFalse((self.root / "out").exists())


class CheckedDefaultMeasurementTests(unittest.TestCase):
    """Every native path and clock is synthetic; none of these are measurements."""
    setUp = CheckedDefaultAcceptanceTests.setUp
    generate = CheckedDefaultAcceptanceTests.generate
    manifest = CheckedDefaultAcceptanceTests.manifest
    load_manifest = CheckedDefaultAcceptanceTests.load_manifest

    def fixture(self, full=False, allocations=False):
        inventory = acceptance.generate(self.inputs, units=(1000, 4000) if full else (1, 2), allocations=allocations)
        path, manifest = self.manifest(purpose="measurement")
        manifest["quiet_machine_approved"] = True
        manifest["execution_environment"] = {"COMPILER_PATH": "/fixture/mold", "PLENTY_LINKER_THREADS": "1"}
        for build in manifest["builds"].values():
            build["compiler_profile"] = "release"
        acceptance.write_json(path, manifest)
        return inventory, path, manifest, acceptance.digest((self.inputs / "inventory.json").read_bytes())

    @staticmethod
    def elf(symbol_name="__plenty_fn_kernel"):
        names = b"\0.shstrtab\0.text\0.strtab\0.symtab\0"
        strings = b"\0" + symbol_name.encode() + b"\0"
        header = struct.pack("<16sHHIQQQIHHHHHH", b"\x7fELF\x02\x01\x01" + b"\0" * 9,
                             2, 62, 1, 0, 0, 64, 0, 64, 0, 0, 64, 5, 1)
        start = 64 + 5 * 64
        sections = [bytes(64), struct.pack("<IIQQQQIIQQ", 1, 3, 0, 0, start, len(names), 0, 0, 1, 0),
                    struct.pack("<IIQQQQIIQQ", 11, 1, 6, 4096, start + len(names), 4, 0, 0, 16, 0),
                    struct.pack("<IIQQQQIIQQ", 17, 3, 0, 0, start + len(names) + 4, len(strings), 0, 0, 1, 0),
                    struct.pack("<IIQQQQIIQQ", 25, 2, 0, 0, start + len(names) + 4 + len(strings), 24, 3, 0, 8, 24)]
        return header + b"".join(sections) + names + b"\x90\x90\x90\xc3" + strings + struct.pack("<IBBHQQ", 1, 2, 0, 2, 4096, 0)

    @staticmethod
    def raw(command, code=0, stdout="", stderr="", **extra):
        return dict(command=command, returncode=code, timeout=False, spawn_error=None, stdout=stdout, stderr=stderr,
                    stdout_base64=base64.b64encode(stdout.encode()).decode(), stderr_base64=base64.b64encode(stderr.encode()).decode(), **extra)

    def simulator(self, inventory, wrong_sample=None, fail_extract=False):
        entries = inventory["entries"]
        self.timed_calls = []
        def run(command, env, timeout, timed=False):
            self.assertEqual(env["PLENTY_CACHE"], "off")
            self.assertNotIn("PLENTY_PROFILE", env)
            self.assertFalse(any(key.startswith("HOTPATH_") for key in env))
            compiler_command = command
            if command[0] == "/usr/bin/time":
                Path(command[5]).write_text("0.10 0.01 2048 3 4\n")
                compiler_command = command[6:]
            if "--compile" in compiler_command or "--emit-object" in compiler_command:
                source = next(arg for arg in compiler_command if arg.endswith(".plenty"))
                family = Path(source).stem
                primary = "work" + str(int(family.removeprefix("corpus-")) - 1) if family.startswith("corpus-") else "kernel"
                Path(compiler_command[-1]).write_bytes(self.elf("__plenty_fn_" + primary))
                row = self.raw(command)
            elif "--check" in compiler_command:
                row = self.raw(command)
            elif "--dump" in compiler_command:
                destination = Path(compiler_command[-1])
                destination.mkdir(parents=True)
                primary = compiler_command[compiler_command.index("--dump-filter") + 1] if "--dump-filter" in compiler_command else "kernel"
                for name in ("index.txt", "ops.txt", "clif.txt", "clif-opt.txt", "asm.txt"):
                    (destination / name).write_text("synthetic unit-test dump __plenty_fn_" + primary + "\n")
                row = self.raw(command)
            elif command[0] in ("readelf", "objdump"):
                text = "fixture extraction"
                if command[0] == "objdump":
                    symbol = next(arg.split("=", 1)[1] for arg in command if arg.startswith("--disassemble="))
                    text = f"00000000001000 <{symbol}>:\n  1000: ret\n"
                row = self.raw(command, code=1 if fail_extract else 0, stdout=text)
            else:
                executable = Path(command[0])
                family = executable.name
                if family == "program":
                    stdout = "1004057\n" if "1000" in self.current_source else "16016057\n"
                    row = self.raw(command, stdout=stdout)
                else:
                    mode = executable.parts[-3]
                    entry = next(e for e in entries if e["mode"] == mode and Path(e["source"]).stem == family)
                    expected = next(c for c in entry["cases"] if c["args"] == command[1:])
                    row = (self.raw(command, code=1, stderr=f"error: {family}.plenty:3: integer overflow\n")
                           if expected["expected"] == "arithmetic-trap" else self.raw(command, stdout=expected["stdout"]))
            if "--check" in compiler_command or "--compile" in compiler_command or "--emit-object" in compiler_command:
                source = next((arg for arg in compiler_command if arg.endswith(".plenty")), "")
                self.current_source = source
            if timed:
                self.timed_calls.append(command)
                row.update(elapsed_ns=1000 + len(self.timed_calls), user_seconds=.1, sys_seconds=.01)
                if wrong_sample == len(self.timed_calls):
                    row.update(stdout="wrong oracle\n", stdout_base64=base64.b64encode(b"wrong oracle\n").decode())
            return row
        return run

    def invoke(self, inventory, manifest_path, manifest, pin, **kwargs):
        options = dict(phases=["runtime"], families=["tight"], runtime_warmups=0, runtime_samples=2,
                       compiler_warmups=0, compiler_samples=1)
        options.update(kwargs)
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "process_group", side_effect=self.simulator(inventory)):
            return acceptance.measure(self.inputs, pin, manifest_path, self.root / "out", allow_perf=True, **options)

    def test_plan_is_native_free_balanced_complete_and_pins_inventory(self):
        inventory, _, _, pin = self.fixture(full=True)
        with patch.object(acceptance, "process_group", side_effect=AssertionError("native command")):
            plan = acceptance.measurement_plan(self.inputs, pin)
        runtime = [j for j in plan["jobs"] if j["stage"] == "runtime"]
        compiler = [j for j in plan["jobs"] if j["stage"] == "compiler"]
        self.assertEqual(len(runtime), 54 * 35)
        self.assertEqual(len(compiler), 8 * 4 * 18)
        self.assertEqual(len({j["id"] for j in plan["jobs"]}), len(plan["jobs"]))
        self.assertEqual(plan["protocol"], "planned-counts")
        self.assertEqual(plan, acceptance.measurement_plan(self.inputs, pin))
        self.assertEqual({j["family"] for j in compiler}, {"corpus-1000", "corpus-4000"})
        self.assertIn("startup", plan["timing_scope"])

    def test_plan_rejects_modified_hash_counts_phases_and_reduced_corpus_before_output(self):
        _, _, _, pin = self.fixture()
        for options in (dict(inventory_sha256="0" * 64), dict(runtime_samples=0), dict(phases=[]), dict(phases=["runtime", "runtime"]),
                        dict(phases=["compiler"]), dict(phases=["code"]), dict(families=["tiny"])):
            args = dict(inputs=self.inputs, inventory_sha256=pin, phases=["runtime"])
            args.update(options)
            with self.subTest(options=options), self.assertRaises(ValueError):
                acceptance.measurement_plan(**args)
        self.assertFalse((self.root / "out").exists())

    def test_plan_cli_materializes_without_native_commands_or_clocks_and_refuses_reuse(self):
        _, _, _, pin = self.fixture()
        args = ["plan", "--inputs", str(self.inputs), "--inventory-sha256", pin, "--output", str(self.root / "plan"), "--phase", "runtime"]
        with patch.object(acceptance, "process_group", side_effect=AssertionError("native")), \
             patch.object(acceptance.time, "perf_counter_ns", side_effect=AssertionError("clock")):
            self.assertEqual(acceptance.main(args), 0)
        self.assertEqual(json.loads((self.root / "plan/plan.json").read_text())["execution"], "not-run; no native commands or clocks")
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            acceptance.main(args)

    def test_actual_manifest_permission_matching_and_owned_path_normalization(self):
        _, path, manifest, _ = self.fixture()
        manifest["builds"]["main"]["source_tree"] = "/owned/main"
        manifest["builds"]["candidate"]["source_tree"] = "/owned/candidate"
        for label in ("main", "candidate"):
            b = manifest["builds"][label]
            b["target_dir"] = "/owned/" + label + "-target"
            b["cargo_flags"] = ["--locked", "--target-dir", b["target_dir"]]
            b["rustflags"] = ["--remap-path-prefix=" + b["source_tree"] + "=/src"]
        acceptance.write_json(path, manifest)
        self.load_manifest(path, True)
        for label in ("main", "candidate"):
            build = manifest["builds"][label]
            build["target_dir"] = build["source_tree"] + "/different-" + label + "-target"
            build["cargo_flags"] = ["--locked", "--target-dir", build["target_dir"]]
            build["build_command"] = ["/home/caleb/.cargo/bin/cargo" if label == "main" else "cargo",
                                      "build", "--locked", "--target-dir", build["target_dir"], "--release"]
        acceptance.write_json(path, manifest)
        loaded = self.load_manifest(path, True)  # Nested role roots normalize longest-first.
        self.assertEqual(loaded["builds"]["main"]["build_command"][0], "/home/caleb/.cargo/bin/cargo")
        self.assertEqual(loaded["builds"]["candidate"]["build_command"][0], "cargo")
        self.assertNotEqual(loaded["builds"]["main"]["build_command"], loaded["builds"]["candidate"]["build_command"])
        for field, value in (("rustflags", ""), ("runtime_rustflags", None), ("cargo_features", ["runtime-checks"]),
                             ("compiler_profile", "debug"), ("toolchain", "different"), ("cargo_flags", ["--offline"]),
                             ("target", "different-target"), ("native_linker", "different-tools")):
            changed = json.loads(json.dumps(manifest))
            changed["builds"]["candidate"][field] = value
            acceptance.write_json(path, changed)
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.load_manifest(path, True)
        for key in ("rustflags", "runtime_rustflags", "cargo_features", "compiler_profile", "toolchain"):
            changed = json.loads(json.dumps(manifest))
            del changed["builds"]["candidate"][key]
            acceptance.write_json(path, changed)
            with self.subTest(missing=key), self.assertRaises(ValueError):
                self.load_manifest(path, True)
        manifest["quiet_machine_approved"] = False
        acceptance.write_json(path, manifest)
        with self.assertRaisesRegex(ValueError, "quiet-machine"):
            self.load_manifest(path, True)

    def test_measure_rejects_nonperf_builds_bad_environment_and_overwrite(self):
        inventory, path, manifest, pin = self.fixture()
        for field, value in (("compiler_profile", "debug"), ("cargo_features", ["runtime-checks"]), ("cargo_features", ["profile-alloc"])):
            changed = json.loads(json.dumps(manifest))
            for build in changed["builds"].values():
                build[field] = value
            with patch.object(acceptance, "load_manifest", return_value=changed), self.assertRaisesRegex(ValueError, "performance requires"):
                acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True, phases=["runtime"])
        manifest["execution_environment"]["PLENTY_PROFILE"] = "1"
        with patch.object(acceptance, "load_manifest", return_value=manifest), self.assertRaisesRegex(ValueError, "execution_environment"):
            acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True, phases=["runtime"])
        del manifest["execution_environment"]["PLENTY_PROFILE"]
        self.invoke(inventory, path, manifest, pin, dry_run=True)
        previous = (self.root / "out/plan.json").read_bytes()
        with self.assertRaises(FileExistsError):
            self.invoke(inventory, path, manifest, pin, dry_run=True)
        self.assertEqual((self.root / "out/plan.json").read_bytes(), previous)

    def test_runtime_samples_keep_all_outputs_warmups_and_semantic_ratios(self):
        inventory, path, manifest, pin = self.fixture()
        self.assertTrue(self.invoke(inventory, path, manifest, pin, runtime_warmups=1))
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 5 * 4 * 3)
        self.assertEqual(sum(r["phase"] == "warmup" for r in rows), 20)
        self.assertTrue(all(r["status"] == "passed" for r in rows))
        self.assertTrue(all(base64.b64decode(r["stdout_base64"]).decode() == r["stdout"] for r in rows))
        summary = json.loads((self.root / "out/result.json").read_text())
        self.assertEqual(summary["protocol"], "exploration-not-acceptance")
        self.assertFalse(summary["packet_complete"])
        self.assertTrue(summary["blocked_metrics"])
        for ratio in summary["statistics"]["ratios"]:
            if ratio["case"] in ("early", "late"):
                self.assertIn(ratio["ratio"], ("U/T", "I/C"))
        self.assertEqual({r["semantics"] for r in rows if r["case"] == "early"}, {"trap-process", "handled-error-process"})

    def test_wrong_oracle_blocks_remaining_samples_without_discarding_attempts(self):
        inventory, path, manifest, pin = self.fixture()
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "process_group", side_effect=self.simulator(inventory, wrong_sample=1)):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["runtime"], families=["tight"], runtime_warmups=0, runtime_samples=3))
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 60)
        self.assertEqual(sum(r["status"] == "failed" for r in rows), 1)
        self.assertEqual(sum(r["status"] == "blocked" for r in rows), 2)
        self.assertEqual(rows[0]["stdout"], "wrong oracle\n")
        summary = json.loads((self.root / "out/result.json").read_text())
        failed = [g for g in summary["statistics"]["groups"] if g["status"] == "blocked"]
        self.assertEqual(len(failed), 1)
        self.assertNotIn("wall_ns", failed[0])

    def test_compiler_samples_keep_full_corpus_all_actions_and_oracles(self):
        inventory, path, manifest, pin = self.fixture(full=True)
        self.assertTrue(self.invoke(inventory, path, manifest, pin, phases=["compiler"]))
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 8 * 4)
        self.assertEqual({r["family"] for r in rows}, {"corpus-1000", "corpus-4000"})
        self.assertEqual({r["action"] for r in rows}, {"check", "object-debug", "aot-debug", "aot-release"})
        self.assertTrue(all(r["peak_rss_kib"] == 2048 for r in rows))
        self.assertTrue(all(r["oracle_scope"] for r in rows))
        commands = [json.loads(line) for line in (self.root / "out/commands.jsonl").read_text().splitlines()]
        self.assertEqual(sum(r.get("phase") == "sample-oracle" for r in commands), 32)
        self.assertFalse(any("--release" in r.get("command", []) and "--emit-object" in r.get("command", []) for r in commands))

    def test_code_extraction_labels_ELF_vs_debug_object_and_blocks_failure(self):
        inventory, path, manifest, pin = self.fixture(full=True)
        self.assertTrue(self.invoke(inventory, path, manifest, pin, phases=["code"]))
        summary = json.loads((self.root / "out/result.json").read_text())
        self.assertTrue(all(c["optimized_linked_ELF"]["executable_section_bytes"] == 4 for c in summary["code"]))
        self.assertTrue(all(c["debug_application_object"]["source_function_symbols"][0]["usable_bytes"] is None for c in summary["code"]))
        self.assertFalse(summary["packet_complete"])

    def test_elf_sizes_preserve_zero_symbols_and_reject_malformed_tables(self):
        path = self.root / "fixture.elf"
        path.write_bytes(self.elf())
        sizes = acceptance.elf_sizes(path)
        self.assertEqual(sizes["executable_sections"], [{"name": ".text", "bytes": 4}])
        self.assertEqual(sizes["source_function_symbols"][0]["st_size"], 0)
        self.assertIsNone(sizes["source_function_symbols"][0]["usable_bytes"])
        for data in (b"not ELF", self.elf()[:100], self.elf()[:12] + b"\0" * 52):
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                acceptance.elf_sizes(path)

    def test_distribution_and_raw_failure_groups_do_not_invent_ratios(self):
        stats = acceptance.distribution([1, 2, 3, 4, 100])
        self.assertEqual(stats["median"], 3)
        self.assertEqual(stats["n"], 5)
        self.assertEqual(stats["max"], 100)  # No outlier discard.
        self.assertGreater(stats["p90"], 4)
        for values in ([], [float("nan")], [-1]):
            with self.assertRaises(ValueError):
                acceptance.distribution(values)
        row = dict(stage="runtime", family="calls", case="early", semantics="trap-process", mode="T",
                   phase="sample", round=0, status="failed", timeout=True, elapsed_ns=300)
        summary = acceptance.sample_summary([row])
        self.assertEqual(summary["ratios"], [])
        self.assertNotIn("wall_ns", summary["groups"][0])

    def test_process_group_timeout_kills_descendants_and_preserves_full_streams(self):
        child = unittest.mock.Mock(pid=42, returncode=-9)
        child.communicate.side_effect = [subprocess.TimeoutExpired(["fixture"], 1), (b"\xffpartial", b"error")]
        usage = unittest.mock.Mock(ru_utime=1.0, ru_stime=2.0)
        with patch.object(acceptance.subprocess, "Popen", return_value=child) as popen, \
             patch.object(acceptance.os, "killpg") as kill, \
             patch.object(acceptance.time, "perf_counter_ns", side_effect=[100, 500]), \
             patch.object(acceptance.resource, "getrusage", return_value=usage):
            row = acceptance.process_group(["fixture"], {}, 1, timed=True)
        self.assertTrue(row["timeout"])
        self.assertEqual(row["returncode"], -9)
        self.assertEqual(row["elapsed_ns"], 400)
        self.assertEqual(base64.b64decode(row["stdout_base64"]), b"\xffpartial")
        kill.assert_called_once_with(42, acceptance.signal.SIGKILL)
        self.assertTrue(popen.call_args.kwargs["start_new_session"])

    def test_warmup_oracle_failure_blocks_measured_group_but_keeps_the_full_schedule(self):
        inventory, path, manifest, pin = self.fixture()
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "process_group", side_effect=self.simulator(inventory, wrong_sample=1)):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["runtime"], families=["tight"], runtime_warmups=1, runtime_samples=2))
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 60)
        self.assertEqual(rows[0]["phase"], "warmup")
        self.assertEqual(rows[0]["status"], "failed")
        self.assertEqual(sum(row["status"] == "blocked" for row in rows), 2)
        self.assertTrue(all(row["phase"] == "sample" for row in rows if row["status"] == "blocked"))

    def test_failed_check_blocks_dependent_measurements_not_compile_error_samples(self):
        inventory, path, manifest, pin = self.fixture()
        normal = self.simulator(inventory)
        def fail(command, env, timeout, timed=False):
            if "--check" in command:
                return self.raw(command, code=1, stderr="fixture diagnostic")
            return normal(command, env, timeout, timed)
        with patch.object(acceptance, "load_manifest", return_value=manifest), patch.object(acceptance, "process_group", side_effect=fail):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["runtime"], families=["tight"], runtime_warmups=0, runtime_samples=1))
        self.assertEqual(self.timed_calls, [])
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 20)
        self.assertTrue(all(r["status"] == "blocked" for r in rows))
        self.assertTrue(all("elapsed_ns" not in r for r in rows))

    def test_measurement_preserves_optimized_native_retention_as_a_blocking_precondition(self):
        inventory, path, manifest, pin = self.fixture()
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "process_group", side_effect=self.simulator(inventory)):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["runtime"], families=["calls"], runtime_warmups=0, runtime_samples=1))
        self.assertEqual(self.timed_calls, [])  # Mock disassembly has no leaf call.
        qualifications = [json.loads(line) for line in (self.root / "out/qualification/results.jsonl").read_text().splitlines()]
        self.assertEqual(sum(r["classification"] == "retention-blocked" for r in qualifications), 4)
        samples = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        self.assertEqual(len(samples), 16)
        self.assertTrue(all(r["status"] == "blocked" for r in samples))

    def test_all_profiler_presence_controls_are_removed_not_set_to_zero(self):
        inventory, path, manifest, pin = self.fixture()
        normal = self.simulator(inventory)
        def isolated(command, env, timeout, timed=False):
            self.assertFalse(any(key.startswith(("PLENTY_PROFILE", "HOTPATH_")) for key in env))
            self.assertEqual(env["COMPILER_PATH"], "/fixture/mold")
            self.assertEqual(env["PLENTY_LINKER_THREADS"], "1")
            self.assertEqual(env["LC_ALL"], "C")
            return normal(command, env, timeout, timed)
        with patch.dict(acceptance.os.environ, {"PLENTY_PROFILE": "0", "PLENTY_PROFILE_EXTRA": "1", "HOTPATH_UPLOAD": "1"}), \
             patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "process_group", side_effect=isolated):
            self.assertTrue(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["runtime"], families=["tight"], runtime_warmups=0, runtime_samples=1))
        qualification_env = json.loads((self.root / "out/qualification/environment.json").read_text())
        self.assertEqual(qualification_env["COMPILER_PATH"], "/fixture/mold")
        self.assertEqual(qualification_env["LC_ALL"], "C")

    def test_failed_extraction_blocks_dependent_runtime_and_reports_code_failure(self):
        inventory, path, manifest, pin = self.fixture(full=True)
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "process_group", side_effect=self.simulator(inventory, fail_extract=True)):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["runtime", "code"], families=["tight"], runtime_warmups=0, runtime_samples=1))
        self.assertEqual(self.timed_calls, [])
        result = json.loads((self.root / "out/result.json").read_text())
        self.assertEqual(result["phase_status"], {"code": "failed", "runtime": "failed"})
        self.assertFalse(result["supported_phases_passed"])

    def test_postrun_integrity_failure_suppresses_statistics_without_erasing_raw_samples(self):
        inventory, path, manifest, pin = self.fixture()
        original = acceptance.pinned_inventory
        calls = 0
        def pinned(*args):
            nonlocal calls
            calls += 1
            if calls == 3:
                raise ValueError("fixture late mutation")
            return original(*args)
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "pinned_inventory", side_effect=pinned), \
             patch.object(acceptance, "process_group", side_effect=self.simulator(inventory)):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["runtime"], families=["tight"], runtime_warmups=0, runtime_samples=1))
        result = json.loads((self.root / "out/result.json").read_text())
        self.assertIn("late mutation", result["integrity_error"])
        self.assertIsNone(result["statistics"])
        self.assertEqual(len((self.root / "out/samples.jsonl").read_text().splitlines()), 20)

    def checks_fixture(self, inventory, manifest, pin, only_i=False):
        root = self.root / "checks-evidence"
        root.mkdir()
        checks = json.loads(json.dumps(manifest))
        for build in checks["builds"].values():
            build.update(compiler_profile="debug", cargo_features=["runtime-checks"])
        acceptance.write_json(root / "build-manifest.json", checks)
        acceptance.write_json(root / "input-inventory.json", inventory)
        acceptance.write_json(root / "validation.json", {"inputs": str(self.inputs), "inventory_sha256": pin})
        acceptance.write_json(root / "result.json", {"passed": True, "dry_run": False})
        rows = []
        for entry in inventory["entries"]:
            if not entry["requires_runtime_checks"] or only_i and entry["mode"] != "I":
                continue
            binary = checks["builds"]["main" if entry["mode"] == "C" else "candidate"]["binary"]
            source = str(self.inputs / entry["source"])
            for profile in ("debug", "release"):
                exe = root / entry["mode"] / profile / Path(source).stem
                exe.parent.mkdir(parents=True, exist_ok=True)
                exe.write_bytes(b"synthetic instrumented executable")
                common = dict(mode=entry["mode"], profile=profile, source=entry["source"])
                rows.append(dict(self.raw([binary, "--check", source]), **common, phase="check", classification="passed"))
                command = [binary] + (["--release"] if profile == "release" else []) + ["--no-cache", "--no-strip", "--compile", source, "-o", str(exe)]
                rows.append(dict(self.raw(command), **common, phase="build", classification="passed"))
                for expected in entry["cases"]:
                    if expected["expected"] == "allocation-control":
                        row = self.raw([str(exe)], code=1, stdout="__test_begin_no_allocations__\n",
                                       stderr="error: unexpected successful allocation in allocation-free region\n")
                    else:
                        row = self.raw([str(exe), *expected["args"]], stdout=expected["stdout"])
                    rows.append(dict(row, **common, phase="runtime", case=expected["name"], classification="unit fixture; reclassification required"))
        (root / "results.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))
        self.pin_checks(root)
        return root, checks

    def pin_checks(self, root):
        self.evidence_index = self.root / "artifact-index.json"
        records = [{"path": str(p), "sha256": acceptance.digest(p.read_bytes()), "size_bytes": p.stat().st_size}
                   for p in root.rglob("*") if p.is_file()]
        acceptance.write_json(self.evidence_index, records)
        self.evidence_pin = acceptance.digest(self.evidence_index.read_bytes())  # Synthetic parent approval only.

    def test_allocation_import_requires_both_modes_profiles_and_positive_controls(self):
        inventory, _, manifest, pin = self.fixture(allocations=True)
        root, checks = self.checks_fixture(inventory, manifest, pin)
        with patch.object(acceptance, "load_manifest", return_value=checks):
            evidence = acceptance.allocation_evidence(root, self.inputs, pin, manifest, self.evidence_index, self.evidence_pin)
        self.assertEqual(evidence["status"], "qualified")
        self.assertEqual(len(evidence["executable_hashes"]), 8)
        self.assertIn("not absolute totals", evidence["scope"])
        self.assertNotIn("allocation_count", evidence)
        rows = (root / "results.jsonl").read_text().splitlines()
        (root / "results.jsonl").write_text("\n".join(row for row in rows if json.loads(row)["mode"] == "I") + "\n")
        self.pin_checks(root)  # Approve the intentionally incomplete fixture, not an unpinned mutation.
        with patch.object(acceptance, "load_manifest", return_value=checks), self.assertRaisesRegex(ValueError, "allocation matrix"):
            acceptance.allocation_evidence(root, self.inputs, pin, manifest, self.evidence_index, self.evidence_pin)

    def test_allocation_import_rejects_old_pins_inert_controls_raw_tampering_and_mismatched_inventory(self):
        inventory, _, manifest, pin = self.fixture(allocations=True)
        root, checks = self.checks_fixture(inventory, manifest, pin)
        original = (root / "results.jsonl").read_text()
        for mutation in ("pin", "features", "raw", "control", "timeout", "source", "profile"):
            changed = json.loads(json.dumps(checks))
            rows = [json.loads(line) for line in original.splitlines()]
            if mutation == "pin":
                changed["builds"]["main"]["source_sha"] = "c" * 40
            elif mutation == "features":
                changed["builds"]["main"]["cargo_features"] = []
            elif mutation in ("raw", "control"):
                row = next(r for r in rows if r["phase"] == "runtime" and r["source"].endswith("allocation-control.plenty"))
                if mutation == "raw":
                    row["stderr_base64"] = ""
                else:
                    row.update(returncode=0, stderr="", stderr_base64="")
            elif mutation == "timeout":
                rows[0]["timeout"] = True
            elif mutation == "source":
                rows[0]["command"][-1] = "/other/source.plenty"
            else:
                row = next(r for r in rows if r["phase"] == "build" and r["profile"] == "release")
                row["command"].remove("--release")
            (root / "results.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))
            self.pin_checks(root)  # Independently exercise semantic rejection after identity validation.
            with self.subTest(mutation=mutation), patch.object(acceptance, "load_manifest", return_value=changed), self.assertRaises(ValueError):
                acceptance.allocation_evidence(root, self.inputs, pin, manifest, self.evidence_index, self.evidence_pin)
        (root / "input-inventory.json").write_text("{}")
        self.pin_checks(root)
        with patch.object(acceptance, "load_manifest", return_value=checks), self.assertRaisesRegex(ValueError, "inventory"):
            acceptance.allocation_evidence(root, self.inputs, pin, manifest, self.evidence_index, self.evidence_pin)

    def test_allocation_import_binds_parent_pin_files_and_all_eight_executables(self):
        inventory, _, manifest, pin = self.fixture(allocations=True)
        root, checks = self.checks_fixture(inventory, manifest, pin)
        approved_pin = self.evidence_pin
        paths = [root / n for n in ("validation.json", "input-inventory.json", "build-manifest.json", "result.json", "results.jsonl")]
        paths += [root / m / p / n for m in ("C", "I") for p in ("debug", "release") for n in ("noalloc", "allocation-control")]
        with patch.object(acceptance, "load_manifest", return_value=checks):
            with self.assertRaisesRegex(ValueError, "parent-approved"):
                acceptance.allocation_evidence(root, self.inputs, pin, manifest)
            evidence = acceptance.allocation_evidence(root, self.inputs, pin, manifest, self.evidence_index, approved_pin)
            self.assertEqual(evidence["approved_index"]["sha256"], approved_pin)
            for path in paths:
                original = path.read_bytes()
                path.write_bytes(original + b"same-output replacement")
                with self.subTest(path=path), self.assertRaisesRegex(ValueError, "identity mismatch"):
                    acceptance.allocation_evidence(root, self.inputs, pin, manifest, self.evidence_index, approved_pin)
                path.write_bytes(original)
            # Rewriting evidence AND its index cannot change the externally supplied approval.
            (root / "result.json").write_text('{"passed":true,"dry_run":false,"rewritten":true}')
            self.pin_checks(root)
            with self.assertRaisesRegex(ValueError, "index pin mismatch"):
                acceptance.allocation_evidence(root, self.inputs, pin, manifest, self.evidence_index, approved_pin)

    def test_allocation_import_rechecks_identities_after_semantic_read(self):
        inventory, _, manifest, pin = self.fixture(allocations=True)
        root, checks = self.checks_fixture(inventory, manifest, pin)
        original = acceptance.classify
        def mutate_after_check(*args):
            result = original(*args)
            (root / "C/debug/noalloc").write_bytes(b"same-output replacement")
            return result
        with patch.object(acceptance, "load_manifest", return_value=checks), \
             patch.object(acceptance, "classify", side_effect=mutate_after_check), \
             self.assertRaisesRegex(ValueError, "identity mismatch"):
            acceptance.allocation_evidence(root, self.inputs, pin, manifest, self.evidence_index, self.evidence_pin)

    def test_allocation_measure_finalization_rechecks_approved_index(self):
        inventory, path, manifest, pin = self.fixture(allocations=True)
        root, checks = self.checks_fixture(inventory, manifest, pin)
        original = acceptance.verify_approved_files
        calls = 0
        def verify(*args):
            nonlocal calls
            calls += 1
            if calls == 3:
                self.evidence_index.write_text("[]")
            return original(*args)
        with patch.object(acceptance, "load_manifest", side_effect=lambda p, **kw: checks if Path(p).name == "build-manifest.json" and Path(p).parent == root else manifest), \
             patch.object(acceptance, "verify_approved_files", side_effect=verify):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["allocation"], checks_evidence=root, checks_index=self.evidence_index, checks_index_sha256=self.evidence_pin))
        result = json.loads((self.root / "out/result.json").read_text())
        self.assertEqual(calls, 3)
        self.assertEqual(result["phase_status"]["allocation"], "invalid-integrity")
        self.assertIn("index pin mismatch", result["integrity_error"])
        self.assertIsNone(result["statistics"])

    def test_allocation_measure_finalization_rehashes_evidence_file_and_executable(self):
        inventory, path, manifest, pin = self.fixture(allocations=True)
        root, checks = self.checks_fixture(inventory, manifest, pin)
        original = acceptance.verify_approved_files
        for victim in (root / "result.json", root / "I/release/noalloc"):
            saved = victim.read_bytes()
            calls = 0
            def verify(*args):
                nonlocal calls
                calls += 1
                if calls == 3:
                    victim.write_bytes(saved + b"late replacement")
                return original(*args)
            with self.subTest(victim=victim), \
                 patch.object(acceptance, "load_manifest", side_effect=lambda p, **kw: checks if Path(p).parent == root else manifest), \
                 patch.object(acceptance, "verify_approved_files", side_effect=verify), \
                 patch.object(acceptance, "process_group", side_effect=AssertionError("native command")):
                output = self.root / ("final-" + victim.name)
                self.assertFalse(acceptance.measure(self.inputs, pin, path, output, allow_perf=True,
                    phases=["allocation"], checks_evidence=root, checks_index=self.evidence_index, checks_index_sha256=self.evidence_pin))
            result = json.loads((output / "result.json").read_text())
            self.assertIn(str(victim), result["integrity_error"])
            self.assertIsNone(result["statistics"])
            self.assertEqual(result["phase_status"]["allocation"], "invalid-integrity")
            victim.write_bytes(saved)

    def test_role_value_only_normalizes_real_prefixes_preserving_types_order_and_tokens(self):
        build = {"source_tree": "/owned/main", "target_dir": "/owned/main/target"}
        values = ["/owned/main/src", "--remap-path-prefix=/owned/main=/src", "-Lnative=/owned/main/target/lib",
                  "--flag=\"/owned/main/lib\"", "/owned/main/target", None, "", 7]
        expected = ["<source_tree>/src", "--remap-path-prefix=<source_tree>=/src", "-Lnative=<target_dir>/lib",
                    "--flag=\"<source_tree>/lib\"", "<target_dir>", None, "", 7]
        self.assertEqual(acceptance.role_value(values, build), expected)
        self.assertEqual(acceptance.role_value(list(reversed(values)), build), list(reversed(expected)))
        self.assertEqual(acceptance.role_value("--target-dir /owned/main/target", build), "--target-dir <target_dir>")
        for value in ("/external path /owned/main/lib", "--flag=\"/external path /owned/main/lib\""):
            self.assertEqual(acceptance.role_value([value], build), [value])
            self.assertEqual(acceptance.role_value(value, build), value)
        for value in ("/external/owned/main/lib", "--flag=/external/owned/main/lib", "/external/key=/owned/main/lib",
                      "--flag=/external/key=/owned/main/lib", "prefix/owned/main", "/owned/main-lookalike/lib", "x=/owned/main/lib",
                      "/owned/main=not-owned/lib", "--flag=/owned/main=not-owned/lib", "/owned/main'not-owned",
                      "/owned/main/../external", "--remap-path-prefix=/owned/main/..=/src", "--flag=/external\\ /owned/main/lib"):
            with self.subTest(value=value):
                self.assertEqual(acceptance.role_value(value, build), value)
        for root in ("/", "//", "///"):
            with self.subTest(root=root), self.assertRaises(ValueError):
                acceptance.role_value("/external/lib", {"source_tree": root})
        spaced = {"source_tree": "/owned main"}
        self.assertEqual(acceptance.role_value(["/owned main/lib"], spaced), ["<source_tree>/lib"])
        self.assertEqual(acceptance.role_value('--flag="/owned main/lib"', spaced), '--flag="<source_tree>/lib"')
        _, path, manifest, _ = self.fixture()
        for label in ("main", "candidate"):
            manifest["builds"][label]["source_tree"] = "/owned/" + label
            manifest["builds"][label]["rustflags"] = ["/external/owned/" + label + "/lib"]
        acceptance.write_json(path, manifest)
        with self.assertRaisesRegex(ValueError, "settings do not match"):
            self.load_manifest(path, True)

    def test_same_output_replacement_after_qualification_blocks_runtime_and_all_statistics(self):
        inventory, path, manifest, pin = self.fixture()
        original_validate = acceptance.validate
        victim = self.root / "out/qualification/I/release/tight"
        def replace_after_qualification(*args, **kwargs):
            result = original_validate(*args, **kwargs)
            victim.write_bytes(b"malicious constant-output executable; unchanged runtime oracle")
            return result
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "validate", side_effect=replace_after_qualification), \
             patch.object(acceptance, "process_group", side_effect=self.simulator(inventory)):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["runtime"], families=["tight"], runtime_warmups=1, runtime_samples=2))
        self.assertFalse(any(command[0] == str(victim) for command in self.timed_calls))
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 60)
        blocked = [r for r in rows if r["mode"] == "I"]
        self.assertTrue(all(r["status"] == "blocked" for r in blocked))
        self.assertTrue(all(str(victim) in r["qualified_artifact_hashes"] for r in rows if r["mode"] == "I"))
        result = json.loads((self.root / "out/result.json").read_text())
        self.assertIsNone(result["statistics"])
        self.assertIn("identity mismatch", result["integrity_error"])
        self.assertTrue(all(s == "invalid-integrity" for s in result["phase_status"].values()))

    def test_artifact_change_during_last_runtime_sample_keeps_raw_row_but_invalidates_packet(self):
        inventory, path, manifest, pin = self.fixture()
        normal = self.simulator(inventory)
        def mutate(command, env, timeout, timed=False):
            row = normal(command, env, timeout, timed)
            if timed and len(self.timed_calls) == 20:
                Path(command[0]).write_bytes(b"same output replacement after last execution")
            return row
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "process_group", side_effect=mutate):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["runtime"], families=["tight"], runtime_warmups=0, runtime_samples=1))
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 20)
        self.assertIn("stdout_base64", rows[-1])
        self.assertEqual(rows[-1]["status"], "failed")
        self.assertIn("artifact_integrity_error", rows[-1])
        self.assertIsNone(json.loads((self.root / "out/result.json").read_text())["statistics"])

    def test_compiler_sample_stream_contract_rejects_each_action_warmup_and_sample(self):
        inventory, path, manifest, pin = self.fixture(full=True)
        for action in ("check", "object-debug", "aot-debug", "aot-release"):
            for phase in ("warmup", "sample"):
                for stream in ("stdout", "stderr"):
                    output = self.root / (action + "-" + phase + "-" + stream)
                    plan = acceptance.measurement_plan(self.inputs, pin, phases=["compiler"], compiler_warmups=1, compiler_samples=2)
                    target = next(j for j in plan["jobs"] if j["action"] == action and j["phase"] == phase)
                    normal = self.simulator(inventory)
                    timed_index = 0
                    def wrong_stream(command, env, timeout, timed=False):
                        nonlocal timed_index
                        row = normal(command, env, timeout, timed)
                        if timed:
                            job = plan["jobs"][timed_index]
                            timed_index += 1
                            # All jobs preceding the single corruption run;
                            # later blocked jobs need no further injection.
                            if job["id"] == target["id"]:
                                row[stream] = "unexpected compiler output\n"
                                row[stream + "_base64"] = base64.b64encode(row[stream].encode()).decode()
                        return row
                    with self.subTest(action=action, phase=phase, stream=stream), \
                         patch.object(acceptance, "load_manifest", return_value=manifest), \
                         patch.object(acceptance, "process_group", side_effect=wrong_stream):
                        self.assertFalse(acceptance.measure(self.inputs, pin, path, output, allow_perf=True,
                            phases=["compiler"], compiler_warmups=1, compiler_samples=2))
                    rows = [json.loads(line) for line in (output / "samples.jsonl").read_text().splitlines()]
                    self.assertEqual(len(rows), 96)
                    failed = next(r for r in rows if r["id"] == target["id"])
                    self.assertEqual(failed["status"], "failed")
                    self.assertEqual(failed[stream], "unexpected compiler output\n")
                    self.assertIn("compiler_stream_error", failed)
                    self.assertTrue(any(r["status"] == "blocked" for r in rows))
                    stats = json.loads((output / "result.json").read_text())["statistics"]
                    group = next(g for g in stats["groups"] if g["family"] == target["family"] and g["case"] == action and g["mode"] == target["mode"])
                    self.assertNotIn("wall_ns", group)

    def test_compiler_stream_contract_must_be_established_outside_intervals(self):
        inventory, path, manifest, pin = self.fixture(full=True)
        for action in ("check", "object-debug", "aot-debug", "aot-release"):
            output = self.root / ("preflight-" + action)
            normal = self.simulator(inventory)
            def unexpected_preflight(command, env, timeout, timed=False):
                row = normal(command, env, timeout, timed)
                actual_action = ("check" if "--check" in command else "object-debug" if "--emit-object" in command else
                                 "aot-release" if "--release" in command else "aot-debug")
                if not timed and command[0] in {b["binary"] for b in manifest["builds"].values()} and actual_action == action:
                    row.update(stderr="unexpected untimed compiler diagnostic\n", stderr_base64=base64.b64encode(b"unexpected untimed compiler diagnostic\n").decode())
                return row
            with self.subTest(action=action), patch.object(acceptance, "load_manifest", return_value=manifest), \
                 patch.object(acceptance, "process_group", side_effect=unexpected_preflight):
                self.assertFalse(acceptance.measure(self.inputs, pin, path, output, allow_perf=True,
                    phases=["compiler"], compiler_warmups=0, compiler_samples=1))
            rows = [json.loads(line) for line in (output / "samples.jsonl").read_text().splitlines()]
            self.assertTrue(all(r["status"] == "blocked" for r in rows if r["action"] == action))
            self.assertEqual(len(rows), 32)

    def test_replaced_reference_object_blocks_every_dependent_compiler_action(self):
        inventory, path, manifest, pin = self.fixture(full=True)
        normal = self.simulator(inventory)
        victim = self.root / "out/reference/T/corpus-1000.o"
        replaced = False
        def replace_reference(command, env, timeout, timed=False):
            nonlocal replaced
            row = normal(command, env, timeout, timed)
            if not timed and "--emit-object" in command and Path(command[-1]) != victim and victim.exists() and not replaced:
                victim.write_bytes(b"replaced reference object")
                replaced = True
            return row
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "process_group", side_effect=replace_reference):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["compiler"], compiler_warmups=0, compiler_samples=1))
        self.assertTrue(replaced)
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        self.assertTrue(all(r["status"] == "blocked" for r in rows if r["mode"] == "T" and r["family"] == "corpus-1000"))
        self.assertFalse(any(str(self.inputs / "T/corpus-1000.plenty") in command for command in self.timed_calls))
        self.assertIsNone(json.loads((self.root / "out/result.json").read_text())["statistics"])

    def test_same_output_new_AOT_replacement_cannot_pass_its_new_binary_oracle(self):
        inventory, path, manifest, pin = self.fixture(full=True)
        plan = acceptance.measurement_plan(self.inputs, pin, phases=["compiler"], compiler_warmups=0, compiler_samples=1)
        target = next(j for j in plan["jobs"] if j["action"] == "aot-debug")
        victim = self.root / "out/work" / target["id"] / "program"
        original_read = Path.read_bytes
        replaced = False
        def replace_after_hash_read(path):
            nonlocal replaced
            content = original_read(path)
            if path == victim and not replaced:
                path.write_bytes(b"same-output constant-only executable after snapshot")
                replaced = True
            return content
        normal = self.simulator(inventory)
        invoked = []
        def runner(command, env, timeout, timed=False):
            invoked.append(command)
            return normal(command, env, timeout, timed)
        with patch.object(acceptance, "load_manifest", return_value=manifest), \
             patch.object(acceptance, "process_group", side_effect=runner), \
             patch.object(Path, "read_bytes", replace_after_hash_read):
            self.assertFalse(acceptance.measure(self.inputs, pin, path, self.root / "out", allow_perf=True,
                phases=["compiler"], compiler_warmups=0, compiler_samples=1))
        self.assertTrue(replaced)
        self.assertFalse(any(command[0] == str(victim) for command in invoked))
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        row = next(r for r in rows if r["id"] == target["id"])
        self.assertEqual(row["status"], "failed")
        self.assertIn("artifact_identity", row)
        self.assertEqual(row["stdout"], "")  # Original successful compiler streams retained.
        self.assertIn("oracle_error", row)
        self.assertIsNone(json.loads((self.root / "out/result.json").read_text())["statistics"])

    def test_final_integrity_covers_prior_new_AOT_artifact_and_reference_object(self):
        inventory, path, manifest, pin = self.fixture(full=True)
        plan = acceptance.measurement_plan(self.inputs, pin, phases=["compiler"], compiler_warmups=0, compiler_samples=1)
        early = next(j for j in plan["jobs"] if j["action"] == "aot-debug")
        for kind in ("new-AOT", "reference"):
            output = self.root / kind
            victim = (output / "work" / early["id"] / "program" if kind == "new-AOT" else output / "reference/T/corpus-1000.o")
            normal = self.simulator(inventory)
            def replace_at_end(command, env, timeout, timed=False):
                row = normal(command, env, timeout, timed)
                if timed and len(self.timed_calls) == len(plan["jobs"]):
                    victim.write_bytes(b"same-output late replacement of earlier artifact")
                return row
            with self.subTest(kind=kind), patch.object(acceptance, "load_manifest", return_value=manifest), \
                 patch.object(acceptance, "process_group", side_effect=replace_at_end):
                self.assertFalse(acceptance.measure(self.inputs, pin, path, output, allow_perf=True,
                    phases=["compiler"], compiler_warmups=0, compiler_samples=1))
            rows = [json.loads(line) for line in (output / "samples.jsonl").read_text().splitlines()]
            self.assertEqual(len(rows), 32)
            self.assertTrue(all(r["status"] == "passed" for r in rows))
            result = json.loads((output / "result.json").read_text())
            self.assertIn(str(victim), result["integrity_error"])
            self.assertEqual(result["phase_status"]["compiler"], "invalid-integrity")
            self.assertIsNone(result["statistics"])

    def test_new_compiler_artifacts_and_reference_objects_have_stable_recorded_identities(self):
        inventory, path, manifest, pin = self.fixture(full=True)
        self.assertTrue(self.invoke(inventory, path, manifest, pin, phases=["compiler"]))
        hashes = json.loads((self.root / "out/artifact-identities.json").read_text())
        rows = [json.loads(line) for line in (self.root / "out/samples.jsonl").read_text().splitlines()]
        for row in rows:
            self.assertEqual(len(row["qualified_artifact_hashes"]), 3)
            self.assertTrue(all(hashes[p] == sha for p, sha in row["qualified_artifact_hashes"].items()))
            if row["action"] != "check":
                identity = row["artifact_identity"]
                self.assertEqual(hashes[identity["path"]], identity["sha256"])
                self.assertEqual(acceptance.digest(Path(identity["path"]).read_bytes()), identity["sha256"])
                if row["action"].startswith("aot-"):
                    self.assertEqual(row["oracle_scope"], "new AOT executable")

    def test_elf_sizes_reject_out_of_file_executable_sections_and_incompatible_types(self):
        path = self.root / "fixture.elf"
        original = self.elf()
        text_header = 64 + 2 * 64
        for field_offset, format_, value in ((32, "Q", len(original) + 1), (24, "Q", len(original) - 1),
                                            (24, "Q", len(original) + 20), (4, "I", 8), (4, "I", 3)):
            data = bytearray(original)
            struct.pack_into("<" + format_, data, text_header + field_offset, value)
            path.write_bytes(data)
            with self.subTest(offset=field_offset, value=value), self.assertRaisesRegex(ValueError, "exceeds file|incompatible"):
                acceptance.elf_sizes(path)

    def test_missing_allocation_evidence_is_blocked_not_a_successful_phase(self):
        inventory, path, manifest, pin = self.fixture()
        self.assertFalse(self.invoke(inventory, path, manifest, pin, phases=["allocation"]))
        result = json.loads((self.root / "out/result.json").read_text())
        self.assertEqual(result["phase_status"]["allocation"], "blocked")
        self.assertFalse(result["supported_phases_passed"])
        self.assertFalse(result["packet_complete"])
        self.assertTrue(any("profiling" in metric for metric in result["blocked_metrics"]))

    def test_gnu_time_parser_refuses_missing_nan_and_malformed_metrics(self):
        path = self.root / "resources.txt"
        for text in ("", "nan 1 2 3 4", "1 2 3", "-1 2 3 4 5"):
            path.write_text(text)
            with self.assertRaises(ValueError):
                acceptance.gnu_time_metrics(path)
        path.write_text("0.25 0.1 4096 2 1\n")
        self.assertEqual(acceptance.gnu_time_metrics(path)["peak_rss_kib"], 4096)


if __name__ == "__main__":
    unittest.main()
