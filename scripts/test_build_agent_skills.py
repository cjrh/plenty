import json
from pathlib import Path
import re
import tempfile
import unittest

import build_agent_skills
import check_links


class RewriteLinksTests(unittest.TestCase):
    PAGES = {
        "design/stdlib/json.md": "skills/plenty/references/std/json.md",
        "design/stdlib/signatures.md": "skills/plenty/references/std/signatures.md",
        "design/examples/cheatsheet.md": "skills/plenty/references/cheatsheet.md",
    }

    def rewrite(self, text):
        return build_agent_skills.rewrite_links(text, "design/stdlib/json.md", self.PAGES)

    def test_bundled_targets_become_relative_links(self):
        self.assertEqual(
            self.rewrite("See [the index](signatures.md#stdjson) and [sheet](../examples/cheatsheet.md).\n"),
            "See [the index](signatures.md#stdjson) and [sheet](../cheatsheet.md).\n",
        )

    def test_other_book_targets_keep_only_their_text(self):
        self.assertEqual(
            self.rewrite("Read [`Result` rules](../language/08-errors/index.md#x) now.\n"),
            "Read `Result` rules now.\n",
        )

    def test_remote_fragment_code_and_fences_are_untouched(self):
        text = (
            "[web](https://example.com/a.md) [here](#local) `[x](y.md)`\n"
            "```plenty\nxs = [a](b.md)\n```\n"
        )
        self.assertEqual(self.rewrite(text), text)

    def test_nested_brackets_and_parentheses(self):
        self.assertEqual(
            self.rewrite("[`list[T]` (lists)](../facilities/lists.md#a-b)\n"),
            "`list[T]` (lists)\n",
        )


class BuildTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.out = build_agent_skills.build(Path(cls.temp.name) / "skills", "9.8.7")

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_tree_is_self_contained(self):
        _, checked, missing = check_links.check(self.out, self.out)
        self.assertGreater(checked, 100)
        self.assertEqual(missing, [])

    def test_version_is_stamped_everywhere(self):
        for path in self.out.rglob("*"):
            if path.is_file():
                self.assertNotIn("@VERSION@", path.read_text(), path)
        plugin = json.loads((self.out / ".claude-plugin/plugin.json").read_text())
        self.assertEqual(plugin["version"], "9.8.7")
        self.assertIn("Plenty 9.8.7", (self.out / "skills/plenty/SKILL.md").read_text())

    def test_marketplace_lists_the_root_plugin(self):
        marketplace = json.loads((self.out / ".claude-plugin/marketplace.json").read_text())
        plugin = json.loads((self.out / ".claude-plugin/plugin.json").read_text())
        [entry] = marketplace["plugins"]
        self.assertEqual((entry["name"], entry["source"]), (plugin["name"], "./"))
        # The version belongs in plugin.json only; Claude Code warns on a mismatch.
        self.assertNotIn("version", entry)

    def test_skill_frontmatter_names_its_directory(self):
        for skill in (self.out / "skills").iterdir():
            text = (skill / "SKILL.md").read_text()
            frontmatter = re.match(r"---\n(.*?)\n---\n", text, re.DOTALL)
            self.assertIsNotNone(frontmatter, skill)
            fields = dict(line.split(": ", 1) for line in frontmatter[1].splitlines())
            self.assertEqual(fields["name"], skill.name)
            self.assertTrue(0 < len(fields["description"]) <= 1024)

    def test_generated_pages_omit_contributor_maintenance(self):
        signatures = (self.out / "skills/plenty/references/std/signatures.md").read_text()
        self.assertNotIn("UPDATE_STDLIB_INDEX", signatures)
        self.assertIn("## `std.json`", signatures)
        self.assertTrue(signatures.startswith("<!-- Generated from the Plenty 9.8.7 book"))

    def test_every_bundled_book_page_is_copied(self):
        for output in build_agent_skills.bundled_pages().values():
            self.assertTrue((self.out / output).is_file(), output)

    def test_refuses_to_replace_an_unrelated_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            (Path(temp) / "keep.txt").write_text("mine")
            with self.assertRaises(SystemExit):
                build_agent_skills.build(Path(temp), "1.0.0")
            self.assertTrue((Path(temp) / "keep.txt").exists())


if __name__ == "__main__":
    unittest.main()
