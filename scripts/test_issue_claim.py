import concurrent.futures
from contextlib import contextmanager
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

from issue_claim import operate


class ClaimTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.db = Path(self.temp.name) / "issues.db"
        with self.connect() as db:
            db.executescript("""
                PRAGMA journal_mode=WAL;
                CREATE TABLE issues(number INTEGER PRIMARY KEY, status TEXT);
                CREATE TABLE comments(number INTEGER PRIMARY KEY,
                    issue_number INTEGER NOT NULL REFERENCES issues(number),
                    author TEXT NOT NULL, body TEXT NOT NULL);
                INSERT INTO issues VALUES (15, 'open'), (16, 'closed');
            """)

    @contextmanager
    def connect(self):
        db = sqlite3.connect(self.db)
        try:
            with db:
                yield db
        finally:
            db.close()

    def claim(self, author="agent-a"):
        return operate(self.db, 15, "claim", author, "Working in /tmp/task-a")

    def test_claim_survives_progress_and_review_comments(self):
        result = self.claim()
        with self.connect() as db:
            db.execute("INSERT INTO comments(issue_number,author,body) VALUES (15,'agent-a','PR ready')")
        for author in ("agent-a", "agent-b"):
            with self.assertRaisesRegex(ValueError, "already claimed"):
                self.claim(author)
        self.assertEqual(operate(self.db, 15, "status")["claims"][0]["number"], result["comment"])

    def test_release_preserves_history_and_allows_new_owner(self):
        first = self.claim()["comment"]
        with self.assertRaisesRegex(ValueError, "owned"):
            operate(self.db, 15, "release", "agent-b", "Taking over", first)
        operate(self.db, 15, "release", "agent-a", "Explicit handoff", first)
        second = self.claim("agent-b")["comment"]
        with self.assertRaisesRegex(ValueError, "active"):
            operate(self.db, 15, "release", "agent-a", "Old release", first)
        self.assertEqual(operate(self.db, 15, "status")["claims"][0]["number"], second)
        with self.connect() as db:
            self.assertEqual(db.execute("SELECT COUNT(*) FROM comments").fetchone()[0], 3)

    def test_closed_missing_and_wrong_database_rejected(self):
        for issue in (16, 99):
            with self.assertRaises(ValueError):
                operate(self.db, issue, "claim", "agent-a", "Work")
        missing = self.db.parent / "missing.db"
        with self.assertRaises(sqlite3.OperationalError):
            operate(missing, 15, "claim", "agent-a", "Work")
        self.assertFalse(missing.exists())
        with self.assertRaisesRegex(ValueError, "absolute"):
            operate("issues.db", 15, "status")

    def test_closed_issue_can_release_its_claim(self):
        claim = self.claim()["comment"]
        with self.connect() as db:
            db.execute("UPDATE issues SET status = 'closed' WHERE number = 15")
        operate(self.db, 15, "release", "agent-a", "Merged and resolved", claim)
        self.assertEqual(operate(self.db, 15, "status")["claims"], [])

    def test_release_comment_from_other_author_does_not_clear_claim(self):
        claim = self.claim()["comment"]
        with self.connect() as db:
            db.execute("INSERT INTO comments(issue_number,author,body) VALUES (15,?,?)",
                       ("agent-b", f"[release #{claim}]\nNot the owner"))
        with self.assertRaisesRegex(ValueError, "already claimed"):
            self.claim("agent-c")

    def test_concurrent_processes_only_one_claim_succeeds(self):
        def attempt(author):
            return subprocess.run(
                [sys.executable, str(Path(__file__).with_name("issue_claim.py")),
                 "claim", "--db", str(self.db), "--issue", "15", "--author", author,
                 "--note", "Concurrent task"], capture_output=True, text=True, timeout=15,
            )
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(attempt, ("agent-a", "agent-b")))
        self.assertEqual(sorted(r.returncode for r in results), [0, 1], results)
        loser = next(r for r in results if r.returncode)
        self.assertIn("already claimed", loser.stderr)
        self.assertEqual(len(operate(self.db, 15, "status")["claims"]), 1)


if __name__ == "__main__":
    unittest.main()
