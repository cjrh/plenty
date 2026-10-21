"""Opt-in end-to-end tests of the compiled Plenty repository search indexer."""

import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest


@unittest.skipUnless(os.environ.get("PLENTY_SEARCH_BIN") and os.environ.get("PLENTY_SQLITE_VEC"),
                     "set PLENTY_SEARCH_BIN and PLENTY_SQLITE_VEC")
class RepositoryIndex(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        self.write("README.md", "Owned resources are released at scope exit.")
        self.write("BACKLOG.md", "duplicate backlog alias")
        self.write("vendor/README.md", "third party text")
        self.write("src/lib.rs", '''//! Module documentation.
const FIXTURE: &str = r#"
/// Not a real doc comment.
"#;
/// Function documentation.
pub fn release() {}
/// Class-like type documentation.
pub struct Owner;
''')
        self.write("script.py", '''"""Python module documentation."""
fixture = """
def pretend():
    fake docs
"""
class Owner:
    """Python class documentation."""
    def release(
        self,
    ):
        """Python function documentation."""
        return 0
''')
        self.write("source.plenty", '''# Plenty module documentation.
import std.json
# Plenty class documentation.
class Owner:
    value: i64
    # Plenty method documentation.
    def get(self) -> i64:
        """Plenty method docstring."""
        self.value
''')
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.root), "-c", "user.name=Search test",
                        "-c", "user.email=search@example.invalid", "commit", "-qm", "fixture"], check=True)
        self.issues = self.root / "issues.db"
        with sqlite3.connect(self.issues) as db:
            db.executescript("CREATE TABLE issues(number INTEGER PRIMARY KEY, title TEXT, details TEXT);"
                             "CREATE TABLE pull_requests(number INTEGER PRIMARY KEY, title TEXT, details TEXT);"
                             "CREATE TABLE comments(body TEXT);")
            db.execute("INSERT INTO issues VALUES(1, 'A defect', 'Issue description')")
            db.execute("INSERT INTO pull_requests VALUES(2, 'A fix', 'PR description')")
            db.execute("INSERT INTO comments VALUES('Discussion must not be indexed')")
        self.helper = self.root / "fake_embed.py"
        self.write("fake_embed.py", "import json, sys\ntexts = json.load(sys.stdin)\n"
                   "assert len(texts) <= 128\njson.dump([[1,0] for _ in texts], sys.stdout)\n")
        self.database = self.root / "search.db"
        self.invoke("init", "2")

    def write(self, name, text):
        destination = self.root / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(text)

    def invoke(self, *args, success=True):
        result = subprocess.run([os.environ["PLENTY_SEARCH_BIN"], os.environ["PLENTY_SQLITE_VEC"],
                                 str(self.database), "fixture-model-v1", *map(str, args)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result.stdout + result.stderr

    def sync(self, success=True, issues=None):
        return self.invoke("sync", self.root, issues or self.issues, sys.executable,
                           self.helper, "unused-cache", success=success)

    def bodies(self):
        with sqlite3.connect(self.database) as db:
            return dict(db.execute("SELECT d.id, coalesce(group_concat(c.body, ''), '') FROM documents d "
                                   "LEFT JOIN chunks c ON c.document_id=d.id GROUP BY d.id"))

    def test_extraction_incremental_changes_and_pruning(self):
        self.assertIn("6 updated", self.sync())
        before = self.database.read_bytes()
        self.assertIn("0 added, 0 changed, 0 deleted", self.invoke("check", self.root, self.issues))
        self.assertEqual(before, self.database.read_bytes())
        bodies = self.bodies()
        self.assertEqual(set(bodies), {"repo:doc:README.md", "repo:source:src/lib.rs",
                                      "repo:source:script.py", "repo:source:source.plenty",
                                      "repo:issue:1", "repo:pr:2"})
        for expected in ("Module documentation", "Function documentation", "Class-like type documentation"):
            self.assertIn(expected, bodies["repo:source:src/lib.rs"])
        self.assertNotIn("Not a real", bodies["repo:source:src/lib.rs"])
        for expected in ("Python module", "Python class", "Python function"):
            self.assertIn(expected, bodies["repo:source:script.py"])
        self.assertNotIn("fake docs", bodies["repo:source:script.py"])
        for expected in ("Plenty module", "Plenty class", "Plenty method documentation", "Plenty method docstring"):
            self.assertIn(expected, bodies["repo:source:source.plenty"])
        self.assertIn("Issue description", bodies["repo:issue:1"])
        self.assertIn("PR description", bodies["repo:pr:2"])
        self.assertNotIn("Discussion", "".join(bodies.values()))
        # An unchanged sync must never need a working inference process.
        self.write("fake_embed.py", "import sys\nsys.exit('inference unavailable')\n")
        self.assertIn("0 updated, 6 unchanged, 0 deleted", self.sync())
        self.write("README.md", "Changed documentation.")
        (self.root / "script.py").unlink()
        before = self.database.read_bytes()
        stale = self.invoke("check", self.root, self.issues, success=False)
        self.assertIn("changed repo:doc:README.md", stale)
        self.assertIn("deleted repo:source:script.py", stale)
        self.assertEqual(before, self.database.read_bytes())
        self.assertIn("inference unavailable", self.sync(success=False))
        self.assertEqual(self.bodies(), bodies)
        self.write("fake_embed.py", "import json, sys\njson.dump([[0,1] for _ in json.load(sys.stdin)], sys.stdout)\n")
        with sqlite3.connect(self.issues) as db:
            db.execute("UPDATE issues SET details='Updated issue description' WHERE number=1")
        self.assertIn("2 updated, 3 unchanged, 1 deleted", self.sync())
        self.assertNotIn("repo:source:script.py", self.bodies())
        self.assertIn("Changed documentation", self.bodies()["repo:doc:README.md"])
        self.assertIn("Updated issue description", self.bodies()["repo:issue:1"])
        self.write("new.md", "A newly tracked document.")
        subprocess.run(["git", "-C", str(self.root), "add", "new.md"], check=True)
        self.assertIn("added repo:doc:new.md", self.invoke("check", self.root, self.issues, success=False))
        self.sync()
        self.invoke("check", self.root, self.issues)
        self.invoke("find", "cleanup", sys.executable, self.helper, "unused-cache")
        with sqlite3.connect(self.database) as db:
            self.assertEqual(db.execute("PRAGMA integrity_check").fetchone()[0], "ok")
            self.assertEqual(db.execute("PRAGMA foreign_key_check").fetchall(), [])
            self.assertEqual(db.execute("SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH 'Python'").fetchone()[0], 0)

    def test_rejects_stale_issue_register_and_preserves_manual_documents(self):
        self.assertIn("primary issues database", self.sync(success=False, issues=self.root / "stale.db"))
        self.write("chunks.json", json.dumps([{"text": "Manual", "embedding": [1, 0]}]))
        self.invoke("put", "manual", "Manual", self.root / "chunks.json")
        self.sync()
        self.assertIn("manual", self.bodies())

    def test_large_document_batches_and_empty_document(self):
        self.write("README.md", "Borrowed values must not outlive their owners.\n" * 1200)
        self.write("empty.md", "")
        subprocess.run(["git", "-C", str(self.root), "add", "empty.md"], check=True)
        self.sync()
        self.assertEqual(self.bodies()["repo:doc:empty.md"], "")
        with sqlite3.connect(self.database) as db:
            self.assertGreater(db.execute("SELECT count(*) FROM chunks WHERE document_id='repo:doc:README.md'").fetchone()[0], 128)
            self.assertEqual(db.execute("SELECT root, revision FROM sync_info").fetchone()[0], str(self.root))


if __name__ == "__main__":
    unittest.main()
