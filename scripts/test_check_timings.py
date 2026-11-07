"""Timing accounting, comparisons and failure preservation without a compiler build."""
import contextlib
import io
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import check_timings as timings


def item(name, start=0, end=10, status="success", **kwargs):
    return {"name": name, "started_at": f"2026-11-01T00:00:{start:02d}Z",
            "completed_at": f"2026-11-01T00:00:{end:02d}Z" if end is not None else None,
            "status": "completed" if end is not None else "in_progress",
            "conclusion": status, **kwargs}


class Reports(unittest.TestCase):
    def ci(self):
        return timings.ci_report([
            {"jobs": [item("shard 1", steps=[item("Compile", end=4), item("Tests", start=4)])]},
            {"jobs": [item("shard 2", end=20, steps=[item("Compile", end=8),
                                                     item("Tests", start=8, end=20, status="failure")]),
                      item("Check timing report", end=40)]},
        ], "run 42", "abc")

    def test_parallel_job_times_are_not_added_to_elapsed(self):
        data = self.ci()
        self.assertEqual(data["elapsed_seconds"], 20)
        self.assertEqual(sum(row["seconds"] for row in data["rows"]), 30)
        self.assertEqual(len(data["jobs"]), 2)
        report = timings.render(data)
        self.assertIn("**20.0s**", report)
        self.assertIn("**30.0s**", report)
        self.assertIn("| Tests | 2 | failure, success | 18.0 | 60.0% |", report)

    def test_unfinished_and_skipped_steps_are_not_fabricated(self):
        skipped = {"name": "Skipped", "status": "completed", "conclusion": "skipped",
                   "started_at": "0001-01-01T00:00:00Z", "completed_at": "0001-01-01T00:00:00Z"}
        data = timings.ci_report([{"jobs": [item("running", end=None, status=None,
                                                steps=[item("done"), item("running", end=None), skipped])]}], "partial", "abc")
        self.assertIsNone(data["jobs"][0]["seconds"])
        self.assertEqual([row["name"] for row in data["rows"]], ["done"])
        self.assertIn("pending", timings.render(data))

    def test_comparison_marks_matrix_changes_and_removed_steps(self):
        before = self.ci()
        after = self.ci()
        after["rows"][0]["seconds"] += 3
        self.assertIn("+3.0s", timings.render(after, before))
        after["rows"] = [row for row in after["rows"] if row["name"] != "Tests"]
        self.assertIn("count 2 → 0", timings.render(after, before))
        with self.assertRaisesRegex(ValueError, "cannot compare"):
            timings.render(after, {**before, "kind": "local"})

    def test_markdown_escaping_and_empty_report(self):
        data = self.ci()
        data["rows"][0]["name"] = "command | with\nnewline"
        self.assertIn("command \\| with newline", timings.render(data))
        data["rows"] = []
        self.assertIn("**0.0s**", timings.render(data))


class Execution(unittest.TestCase):
    def test_failed_suite_saves_status_and_stops_before_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "run"
            with patch.object(sys, "argv", ["check_timings.py", "suite", "--output", str(out)]), \
                    patch.object(timings.subprocess, "run", return_value=subprocess.CompletedProcess([], 17)) as run, \
                    patch.object(timings, "git_revision", return_value="abc"), \
                    patch.object(timings, "native_cache_environment", side_effect=lambda repo, env: dict(env)), \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(timings.main(), 17)
            self.assertEqual(run.call_count, 1)
            data = json.loads((out / "timings.json").read_text())
            self.assertEqual(data["rows"][0]["status"], "failed (17)")
            self.assertIn("--no-run", data["rows"][0]["command"])
            self.assertTrue((out / "timings.md").is_file())

    def test_suite_keeps_full_workspace_and_features_in_both_phases(self):
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(sys, "argv", ["check_timings.py", "suite", "--profile", "ci", "--output", tmp + "/run"]), \
                    patch.object(timings.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run, \
                    patch.object(timings, "git_revision", return_value="abc"), \
                    patch.object(timings, "native_cache_environment", side_effect=lambda repo, env: dict(env)), \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(timings.main(), 0)
            self.assertEqual(run.call_count, 2)
            for call in run.call_args_list:
                command = call.args[0]
                self.assertEqual(command[:5], ["cargo", "test", "--locked", "--workspace", "--all-features"])
                self.assertIn("ci", command)
            self.assertNotIn("--no-run", run.call_args.args[0])

    def test_local_ci_continues_after_failure_and_keeps_logs_and_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            scripts = root / "scripts"
            scripts.mkdir()
            for name in ("local_ci.sh", "check_timings.py", "native_cache.py"):
                shutil.copy2(timings.REPO / "scripts" / name, scripts / name)
            commands = root / "bin"
            commands.mkdir()
            for name, body in [("cargo", 'if [ "$1" = fmt ]; then echo failed-format; exit 17; fi\necho built'),
                               ("git", "echo abc")]:
                executable = commands / name
                executable.write_text("#!/bin/sh\n" + body + "\n")
                executable.chmod(0o755)
            result = subprocess.run(["bash", str(scripts / "local_ci.sh"), "fmt", "default_build"],
                                    env={**os.environ, "PATH": f"{commands}:{os.environ['PATH']}"},
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stderr)
            reports = list((root / "target/local-ci").glob("run-*/timings.json"))
            self.assertEqual(len(reports), 1)
            data = json.loads(reports[0].read_text())
            self.assertEqual([(row["name"], row["status"]) for row in data["rows"]],
                             [("fmt", "FAILED"), ("default_build", "ok")])
            self.assertIn("failed-format", (reports[0].parent / "fmt.log").read_text())
            self.assertIn("built", (reports[0].parent / "default_build.log").read_text())

    def test_ci_reads_current_attempt_and_all_pages(self):
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(sys, "argv", ["check_timings.py", "ci", "--repo", "owner/repo", "--run", "42", "--output", tmp + "/run"]), \
                    patch.object(timings, "gh_json", side_effect=[{"run_attempt": 3, "head_sha": "abc"}, [{"jobs": []}]]) as gh, \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(timings.main(), 0)
            self.assertEqual(gh.call_args.args, ("repos/owner/repo/actions/runs/42/attempts/3/jobs?per_page=100",))
            self.assertTrue(gh.call_args.kwargs["paginate"])
            self.assertTrue((Path(tmp) / "run/jobs.json").is_file())


if __name__ == "__main__":
    unittest.main()
