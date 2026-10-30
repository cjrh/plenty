import json
from pathlib import Path
import tempfile
import unittest

import skill_trial


def event(kind, *blocks):
    return json.dumps({"type": kind, "message": {"content": list(blocks)}})


class TranscriptTests(unittest.TestCase):
    TRANSCRIPT = "\n".join([
        event("assistant", {"type": "tool_use", "id": "a", "name": "Skill", "input": {"skill": "plenty:plenty"}}),
        event("user", {"type": "tool_result", "tool_use_id": "a", "content": "loaded"}),
        event("assistant", {"type": "tool_use", "id": "b", "name": "Bash",
                            "input": {"command": "plenty --check main.plenty"}}),
        event("user", {"type": "tool_result", "tool_use_id": "b", "is_error": True,
                       "content": [{"type": "text", "text": "error: /tmp/x/main.plenty:2:8: unknown type `int`"}]}),
        event("assistant", {"type": "tool_use", "id": "c", "name": "Bash",
                            "input": {"command": "cat skills/plenty/SKILL.md; plenty main.plenty"}}),
        event("user", {"type": "tool_result", "tool_use_id": "c", "content": "error: no plum"}),
        event("assistant", {"type": "tool_use", "id": "d", "name": "Bash",
                            "input": {"command": "grep plenty notes.txt"}}),
        json.dumps({"type": "result", "result": "done", "num_turns": 3, "total_cost_usd": 0.5}),
    ])

    def test_pairs_calls_with_results(self):
        calls, result, final = skill_trial.tool_calls(self.TRANSCRIPT)
        self.assertEqual([call["tool"] for call in calls], ["Skill", "Bash", "Bash", "Bash"])
        self.assertTrue(calls[1]["failed"])
        self.assertEqual((result["num_turns"], final), (3, "done"))

    def test_counts_compiler_runs_and_diagnostics_only(self):
        calls, _, _ = skill_trial.tool_calls(self.TRANSCRIPT)
        self.assertEqual(len(skill_trial.compiler_calls(calls)), 2)
        # A program's own `error:` output is not a diagnostic.
        self.assertEqual(
            [found["error"] for found in skill_trial.diagnostics(calls)],
            ["main.plenty:2:8: unknown type `int`"],
        )
        self.assertEqual(len(skill_trial.failed_commands(calls)), 1)


class IsolationTests(unittest.TestCase):
    def test_checkout_and_instruction_directories_are_rejected(self):
        self.assertFalse(skill_trial.outside_repository(skill_trial.REPO / "target/tmp"))
        with tempfile.TemporaryDirectory(dir=skill_trial.default_workdir_root()) as temp:
            self.assertTrue(skill_trial.outside_repository(Path(temp)))
            (Path(temp) / "CLAUDE.md").write_text("rules")
            (Path(temp) / "task").mkdir()
            self.assertFalse(skill_trial.outside_repository(Path(temp) / "task"))

    def test_task_names_are_unique_and_checked(self):
        names = [task.name for task in skill_trial.TASKS]
        self.assertEqual(len(names), len(set(names)))
        self.assertTrue(all(task.checks for task in skill_trial.TASKS))


if __name__ == "__main__":
    unittest.main()
