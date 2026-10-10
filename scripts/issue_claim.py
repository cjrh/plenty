#!/usr/bin/env python3
"""Coordinate issue ownership through append-only comments in the shared DB."""

import argparse
import json
from pathlib import Path
import re
import sqlite3
import sys


def active_claims(comments):
    active = {}
    for comment in comments:
        first_line = comment["body"].split("\n", 1)[0]
        if first_line == "[claim]":
            active[comment["number"]] = dict(comment)
        elif match := re.fullmatch(r"\[release #(\d+)\]", first_line):
            number = int(match[1])
            if number in active and active[number]["author"] == comment["author"]:
                del active[number]
    return active


def operate(database, issue, action, author=None, note=None, claim=None):
    path = Path(database)
    if not path.is_absolute():
        raise ValueError("Pass the absolute path to the primary worktree's issues.db.")
    if action not in {"status", "claim", "release"}:
        raise ValueError("Unknown claim action.")
    if action != "status" and (not author or not author.strip() or not note or not note.strip()):
        raise ValueError("Writes require a unique --author and a nonempty --note.")
    mode = "ro" if action == "status" else "rw"
    db = sqlite3.connect(f"{path.resolve().as_uri()}?mode={mode}", uri=True, timeout=5)
    db.row_factory = sqlite3.Row
    try:
        db.execute("PRAGMA foreign_keys = ON")
        # Serialize the ownership check and comment insertion across all workers.
        db.execute("BEGIN" if action == "status" else "BEGIN IMMEDIATE")
        row = db.execute("SELECT status FROM issues WHERE number = ?", (issue,)).fetchone()
        if row is None:
            raise ValueError(f"Issue #{issue} does not exist.")
        comments = db.execute(
            "SELECT number, author, body FROM comments WHERE issue_number = ? ORDER BY number",
            (issue,),
        ).fetchall()
        active = active_claims(comments)
        if action == "status":
            result = {"issue": issue, "status": row["status"], "claims": list(active.values())}
        else:
            if action == "claim":
                if row["status"] != "open":
                    raise ValueError(f"Issue #{issue} is closed.")
                if active:
                    owners = ", ".join(f"#{n} by {c['author']}" for n, c in active.items())
                    raise ValueError(f"Issue #{issue} is already claimed: {owners}. Do not start work.")
                body = f"[claim]\n\n{note.strip()}"
            else:
                if claim not in active or active[claim]["author"] != author:
                    raise ValueError("Release requires an active --claim owned by this --author.")
                body = f"[release #{claim}]\n\n{note.strip()}"
            cursor = db.execute(
                "INSERT INTO comments(issue_number, author, body) VALUES (?, ?, ?)",
                (issue, author, body),
            )
            result = {"issue": issue, "comment": cursor.lastrowid, "action": action, "author": author}
        db.commit()
        return result
    except Exception:
        db.rollback()
        raise
    finally:
        db.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("status", "claim", "release"))
    parser.add_argument("--db", required=True)
    parser.add_argument("--issue", required=True, type=int)
    parser.add_argument("--author")
    parser.add_argument("--note")
    parser.add_argument("--claim", type=int, help="Original claim comment number, for release")
    args = parser.parse_args()
    try:
        result = operate(args.db, args.issue, args.action, args.author, args.note, args.claim)
    except (ValueError, sqlite3.Error) as error:
        parser.exit(1, f"{error}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    sys.exit(main())
