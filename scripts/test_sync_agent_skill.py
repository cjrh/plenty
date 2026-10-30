import json
from pathlib import Path
import re
import tempfile
import unittest

import sync_agent_skill
import check_links


class RewriteLinksTests(unittest.TestCase):
    PAGES = {
        "book/src/design/stdlib/json.md": "skills/plenty/references/std/json.md",
        "book/src/design/stdlib/signatures.md": "skills/plenty/references/std/signatures.md",
        "book/src/design/examples/cheatsheet.md": "skills/plenty/references/cheatsheet.md",
        ".agents/skills/plenty/SKILL.md": "skills/plenty/SKILL.md",
        ".agents/skills/plenty/references/errors.md": "skills/plenty/references/errors.md",
    }

    def rewrite(self, text, page="book/src/design/stdlib/json.md"):
        return sync_agent_skill.rewrite_links(text, page, self.PAGES)

    def test_skill_links_into_the_book_become_bundled_links(self):
        self.assertEqual(
            self.rewrite(
                "[sheet](../../../book/src/design/examples/cheatsheet.md) "
                "[errors](references/errors.md#avoid) "
                "[tour](../../../book/src/tutorial/index.md)\n",
                ".agents/skills/plenty/SKILL.md",
            ),
            "[sheet](references/cheatsheet.md) [errors](references/errors.md#avoid) tour\n",
        )

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
        cls.out = sync_agent_skill.build(Path(cls.temp.name) / "skills", "9.8.7")

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

    def test_licences_match_the_repository(self):
        for licence in ("LICENSE-APACHE", "LICENSE-MIT"):
            self.assertEqual((self.out / licence).read_text(), (sync_agent_skill.REPO / licence).read_text())

    def test_marketplace_lists_the_root_plugin(self):
        marketplace = json.loads((self.out / ".claude-plugin/marketplace.json").read_text())
        plugin = json.loads((self.out / ".claude-plugin/plugin.json").read_text())
        [entry] = marketplace["plugins"]
        self.assertEqual((entry["name"], entry["source"]), (plugin["name"], "./"))
        # The version belongs in plugin.json only; Claude Code warns on a mismatch.
        self.assertNotIn("version", entry)

    def test_only_the_plenty_skill_is_published(self):
        # Repository maintenance skills (consistency-check, upstream-check,
        # plenty-search) live beside it in .agents/skills but are not for users.
        self.assertEqual([p.name for p in (self.out / "skills").iterdir()], ["plenty"])

    def test_skill_frontmatter_names_its_directory(self):
        for skill in (self.out / "skills").iterdir():
            text = (skill / "SKILL.md").read_text()
            frontmatter = re.match(r"---\n(.*?)\n---\n", text, re.DOTALL)
            self.assertIsNotNone(frontmatter, skill)
            fields = dict(line.split(": ", 1) for line in frontmatter[1].splitlines())
            self.assertEqual(fields["name"], skill.name)
            self.assertTrue(0 < len(fields["description"]) <= 1024)

    def test_contributor_passages_and_issue_notes_are_dropped(self):
        signatures = (self.out / "skills/plenty/references/std/signatures.md").read_text()
        self.assertNotIn("UPDATE_STDLIB_INDEX", signatures)
        self.assertIn("## `std.json`", signatures)
        self.assertTrue(signatures.startswith("<!-- Copied from the Plenty 9.8.7 book"))
        for path in self.out.rglob("*.md"):
            text = path.read_text()
            self.assertNotIn("contributors-only", text, path)
            self.assertNotIn("cargo update", text, path)
        # Book pages may cite issues in prose; the skill's own notes are dropped.
        for page, output in sync_agent_skill.published_pages().items():
            if page.startswith(sync_agent_skill.SKILL_SOURCE) and page.endswith(".md"):
                text = (self.out / output).read_text()
                self.assertIsNone(re.search(r"issues? #\d", text), output)
                self.assertNotIn("in this repository", text, output)

    def test_marker_helpers(self):
        self.assertEqual(
            sync_agent_skill.strip_contributor_notes(
                "a\n\n<!-- contributors-only -->\nx\n<!-- /contributors-only -->\n\nb\n", "p"
            ),
            "a\n\nb\n",
        )
        with self.assertRaises(SystemExit):
            sync_agent_skill.strip_contributor_notes("<!-- contributors-only -->\nx\n", "p")
        note = sync_agent_skill.ISSUE_NOTE
        line = sync_agent_skill.ISSUE_NOTE_LINE
        self.assertEqual(note.sub("", line.sub("", "a `x`. <!-- issue #5 -->\n\n<!-- issues #1, #2:\nlater -->\nb\n")),
                         "a `x`.\n\nb\n")

    def test_in_repo_skill_links_resolve(self):
        repo = Path(sync_agent_skill.REPO)
        _, checked, missing = check_links.check(repo, repo / sync_agent_skill.SKILL_SOURCE)
        self.assertGreater(checked, 10)
        self.assertEqual(missing, [])

    def test_every_page_is_copied(self):
        for output in sync_agent_skill.published_pages().values():
            self.assertTrue((self.out / output).is_file(), output)

    def test_sync_into_a_clone_keeps_its_history(self):
        with tempfile.TemporaryDirectory() as temp:
            (Path(temp) / ".git").mkdir()
            (Path(temp) / ".git/HEAD").write_text("ref")
            sync_agent_skill.build(Path(temp), "1.0.0")
            (Path(temp) / "stale.md").write_text("old")
            sync_agent_skill.build(Path(temp), "1.0.0")
            self.assertTrue((Path(temp) / ".git/HEAD").exists())
            self.assertFalse((Path(temp) / "stale.md").exists())

    def test_refuses_to_replace_an_unrelated_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            (Path(temp) / "keep.txt").write_text("mine")
            with self.assertRaises(SystemExit):
                sync_agent_skill.build(Path(temp), "1.0.0")
            self.assertTrue((Path(temp) / "keep.txt").exists())


class RepositorySkillTests(unittest.TestCase):
    def test_maintenance_skills_are_linked_for_claude(self):
        repo = Path(sync_agent_skill.REPO)
        for name in ("consistency-check", "upstream-check"):
            text = (repo / ".agents/skills" / name / "SKILL.md").read_text()
            fields = dict(line.split(": ", 1) for line in re.match(r"---\n(.*?)\n---\n", text, re.DOTALL)[1].splitlines())
            self.assertEqual(fields["name"], name)
            self.assertTrue(0 < len(fields["description"]) <= 1024)
            link = repo / ".claude/skills" / name
            self.assertTrue(link.is_symlink(), link)
            self.assertEqual(str(link.readlink()), f"../../.agents/skills/{name}")


if __name__ == "__main__":
    unittest.main()
