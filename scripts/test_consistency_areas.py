from pathlib import Path
import re
import tempfile
import unittest

import consistency_areas as areas


class AreaOfTests(unittest.TestCase):
    def test_paths_map_to_areas(self):
        cases = {
            "book/src/tutorial/04-collections/dictionaries.md": "tutorial-04-collections",
            "book/src/tutorial/index.md": "tutorial-index",
            "book/src/design/language/04-expressions/assignment.md": "design-language",
            "book/src/design/stdlib/http-client.md": "std-net",
            "stdlib/std/http/client.plenty": "std-net",
            "book/src/design/stdlib/fs/walking.md": "std-system",
            "stdlib/std/linalg.plentyi": "std-data",
            "book/src/design/index.md": "top-level",
            "README.md": "top-level",
            ".agents/skills/plenty/references/errors.md": "skill",
            "tests/test_std_json.rs": "tests-std",
            "tests/test_dictionary_mutation.rs": "tests-other",
            "tests/fixtures/modules/main.plenty": "tests-fixtures",
            "src/frontend/collections.rs": "src-frontend",
            "src/lsp.rs": "src-other",
            "examples/finder/main.plenty": "examples",
        }
        for path, expected in cases.items():
            self.assertEqual(areas.area_of(path), expected, path)

    def test_generated_vendored_history_and_data_are_skipped(self):
        for path in (
            "book/src/design/stdlib/signatures.md",
            "plenty-runtime/vendor/ring-0.17.14/src/lib.rs",
            "book/src/proposals/completed/dicts.md",
            "examples/sales_report/data/sales.csv",
            "issues.db",
        ):
            self.assertIsNone(areas.area_of(path), path)
        self.assertEqual(areas.area_of("book/src/proposals/x.md", include_history=True), "history")

    def test_unmapped_std_module_is_reported(self):
        self.assertEqual(areas.area_of("stdlib/std/brand_new.plenty"), "std-other")


class BuildTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)

    def files(self, contents):
        result = []
        for path, text in contents.items():
            (self.repo / path).parent.mkdir(parents=True, exist_ok=True)
            (self.repo / path).write_text(text)
            result.append((path, len(text.encode())))
        return result

    def test_hits_filter_and_split(self):
        files = self.files({
            "book/src/tutorial/02-values/a.md": "d[k] = v\nok\n",
            "book/src/tutorial/02-values/b.md": "nothing\n",
            "book/src/tutorial/03-absence/c.md": "d[k] = 1\nd[j] = 2\nd[i] = 3\n",
            "tests/test_a.rs": "d[k] = v\n",
        })
        pattern = [re.compile(r"\]\s*=")]
        found = areas.build_areas(files, self.repo, pattern, hits_only=True, max_hits=2)
        self.assertEqual(
            [(a["id"], a["kind"], a["hits"], a["files"]) for a in found],
            [
                ("tests-other", "scan", 1, ["tests/test_a.rs"]),
                ("tutorial-02-values", "read", 1, ["book/src/tutorial/02-values/a.md"]),
                ("tutorial-03-absence", "read", 3, ["book/src/tutorial/03-absence/c.md"]),
            ],
        )
        packed = areas.pack(found, max_hits=4)
        self.assertEqual([a["id"] for a in packed],
                         ["tests-other", "tutorial-02-values..tutorial-03-absence"])
        self.assertEqual(packed[1]["hits"], 4)
        self.assertEqual(len(areas.pack(found, max_hits=3)), 3)

    def test_read_areas_split_by_budget(self):
        files = self.files({f"examples/e{i}.plenty": "x" * 100 for i in range(5)})
        found = areas.build_areas(files, self.repo, budget=250)
        self.assertEqual([a["id"] for a in found], ["examples.1", "examples.2", "examples.3"])
        self.assertEqual(sum(len(a["files"]) for a in found), 5)

    def test_select(self):
        found = [{"id": "tutorial-02"}, {"id": "src-codegen"}, {"id": "skill"}]
        self.assertEqual(areas.select(found, only=["tutorial", "src"], skip=["src-codegen"]),
                         [{"id": "tutorial-02"}])


class RepositoryTests(unittest.TestCase):
    def test_tree_covers_every_tutorial_chapter_and_std_module(self):
        found = areas.build_areas(areas.tracked_files())
        ids = {a["id"].split(".")[0] for a in found}
        chapters = {p.name for p in (areas.REPO / "book/src/tutorial").iterdir() if p.is_dir()}
        self.assertEqual({c for c in chapters if f"tutorial-{c}" not in ids}, set())
        self.assertIn("skill", ids)
        self.assertIn("src-frontend", ids)
        covered = {f for a in found for f in a["files"]}
        for source in (areas.REPO / "stdlib/std").rglob("*.plenty"):
            self.assertIn(source.relative_to(areas.REPO).as_posix(), covered)
        for area in found:
            if area["kind"] == "read":
                self.assertLessEqual(area["bytes"], areas.DEFAULT_BUDGET + 200_000, area["id"])


if __name__ == "__main__":
    unittest.main()
