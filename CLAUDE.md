# Plenty — orientation for coding agents

Plenty is a statically typed language with Python-shaped syntax, single
ownership with checked borrows, and native compilation through Cranelift.
Design goals: low memory use, no hidden allocation, fast compilation, and a
small, understandable language. There is no interpreter, REPL, or JIT.

This file is for fast orientation. The documentation is authoritative; also
follow `AGENTS.md`.

## Worktrees and pull requests

Repository changes are delivered as GitHub PRs in `cjrh/plenty`; the user reviews
and merges them. A change request authorizes committing the scoped changes,
pushing the task branch, and creating/updating its PR unless local-only work was
requested. It does not authorize merging, enabling auto-merge, pushing to the
default branch, or rewriting a published branch.

1. Inspect `git status`, `git worktree list`, and the repository's remote/default
   branch. Fetch before creating a new task branch from the current remote
   default branch. Create a separate worktree for each independent task/worker,
   with a descriptive branch name; continue in an already assigned worktree
   rather than creating duplicate branches or PRs. Keep the main worktree out of
   implementation work. Never reset/stash another worker's changes or include
   unrelated changes in a commit.
2. Establish responsibility for components/files before running workers
   concurrently. If work depends on another PR, state its base/dependency
   explicitly. Avoid unnecessary edits to shared documentation/backlog sections.
   Run commands with an explicit task worktree directory; do not assume another
   worker's files, build outputs, ports, or temporary paths belong to this task.
3. Implement and validate the change with its tests and reference/tutorial
   updates. For Rust changes, format and run the applicable workspace tests and
   lint checks; use `cargo test --workspace` when checking the full suite, since
   the runtime is a separate workspace member. Documentation-only changes need
   relevant document/link/example checks, not an unrelated full compiler build.
4. Review and stage only the task's files, commit, push the task branch, and
   create/update its PR using `gh`. Use a draft until implementation and required
   validation are complete. The PR should explain the problem and final behavior,
   cite applicable backlog items and **SQLite issue #N** explicitly (not GitHub's
   `Fixes #N` unless it is actually a GitHub issue), and report checks, results,
   and material limitations. Use a body file for multiline PR descriptions.
5. Inspect PR checks and address failures caused by the change. Report external
   blockers or checks not run accurately. When updating a published branch with
   the latest default branch, prefer a merge that preserves its history; do not
   force-push without explicit instruction. Resolve conflicts within the task
   worktree and rerun affected checks. Hand off the PR URL and leave merging to
   the user; keep the worktree and branch for review feedback.

The issues database is the deliberate exception to isolated working files: all
workers use the main worktree's database as described below. Do not introduce
another task-status file; planned work/status remains in `book/src/backlog.md`.

## Where things are

| Path | Contents |
|---|---|
| `book/src/design/` | Language reference: current behavior, limits, architecture |
| `book/src/design/04-implementation-status.md` | What works today |
| `book/src/tutorial/` | Learn Plenty: runnable lessons, checked by tests |
| `book/src/backlog.md` | The only list of future work and priorities |
| `book/src/proposals/` | Design rationale and alternatives (may contain old syntax) |
| `src/` | Compiler: frontend (parse, check) and Cranelift codegen |
| `plenty-runtime/` | Rust runtime crate, embedded in the compiler and linked into programs |
| `tests/` | Integration tests, one area per file |
| `examples/` | Small sample programs and a compile-time benchmark |

`book/src/SUMMARY.md` lists every book page in reading order.

## Build & test

| Command | Use |
|---|---|
| `cargo test --workspace` | Run compiler and runtime tests |
| `cargo test --workspace 2>&1 \| grep "test result"` | Summary only; preserve the test exit status when scripting |
| `cargo test --test test_tutorial` | Check the tutorial lessons |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lint compiler and runtime |
| `cargo run -- FILE` | Compile and run a Plenty program (needs `cc`) |
| `cargo run -- --check FILE` | Type-check without linking or running |
| `cargo run -- --compile FILE -o OUT` | Produce a native executable |
| `mdbook serve book --open` | Read the docs in a browser |

Running with no arguments prints all options.

## Keeping things aligned

A learner-visible language change updates, in the same change:

- the implementation and its tests,
- the reference page(s) in `book/src/design/`,
- the tutorial lessons that teach the feature,
- the backlog entry, if its scope changed or it completed.

Tutorial code fences are executable tests, so stale lessons fail the suite.

## Legacy

The original stack-based language survives only behind `--legacy`, as backend
regression coverage. It does not define the current language. Some source
comments still carry `§N.M` references to its removed `DESIGN.md`; treat them as
historical.

## Project conventions

- **Don't add comments that just restate the code.** Comments should give the
  *why*: invariants, non-obvious constraints, links to the reference.
- **Phase-guard tests**: a test that asserts a feature is *rejected* only until
  later work lands should say so in a comment naming the backlog ID, so it is
  deleted when that work completes.
- **Cranelift API** is not indexed by context7. Read the registry source under
  `~/.cargo/registry/src/` for the pinned `cranelift-*` version. We depend on
  the individual crates, not the umbrella `cranelift` crate.

## Issues register

`issues.db` (SQLite) records defects and gaps found in implemented behavior:
crashes, wrong or misleading diagnostics, missing operations a lesson implies,
performance concerns. The backlog stays the only list of planned work and
priorities; when an issue is scheduled, cite `issue #N` in its backlog entry.

### One database shared by every worktree

Always access the `issues.db` in the main **primary worktree**, not the copy Git
checks out in a task worktree. The primary worktree is the first entry reported
by `git worktree list --porcelain`; it is not whichever task branch happens to
be named `main`. Resolve the path once for the session and pass it explicitly
to every SQLite command or script:

```sh
export PLENTY_MAIN_WORKTREE="$(git worktree list --porcelain | sed -n '1s/^worktree //p')"
export PLENTY_ISSUES_DB="$PLENTY_MAIN_WORKTREE/issues.db"
test -n "$PLENTY_MAIN_WORKTREE" && test -f "$PLENTY_ISSUES_DB"
```

Verify that this resolves to the primary checkout before writing. SQLite scripts
should open the existing absolute path (`mode=rw`, or `mode=ro` for readers),
so a wrong path cannot silently create a new register. Do not copy the database
into worktrees, replace the tracked copy with a symlink, or run task-local
`sqlite3 issues.db`. The checkout copy is not the working register.

All workers may update the shared register directly for authorized issue work.
Use a busy timeout and short transactions. For additions, repeat the duplicate
search and INSERT within one write transaction; let SQLite assign `number` and
read the assigned ID rather than computing `MAX(number) + 1`. For edits, read
and update inside a short transaction or compare the original field value in
the UPDATE predicate to avoid overwriting another worker's intervening edit.
Never hold a transaction while researching, building, or waiting on GitHub.

An open fixing PR is not a completed fix in the shared register. Keep the issue
open until the PR has merged; then close it with the actual merged fixing commit
and resolution. Keep shared backlog status consistent with that distinction.

Feature PRs must not stage `issues.db` from their task worktree: doing so can
replace newer shared entries with a stale branch snapshot. Persisting the tracked
register is a separate, coordinated maintenance change using a consistent SQLite
snapshot of the main database and the same PR review policy. Do not copy an active
WAL-mode database with an ordinary file copy. Do not overwrite/reset the live
database when switching/updating the main checkout; preserve newer rows and
reconcile logical changes transactionally. Database persistence must never
discard another worker's additions or edits.

### Schema and commands

Table `issues(number INTEGER PRIMARY KEY, title TEXT, details TEXT, status TEXT, severity TEXT, resolution TEXT)`.
`status` is `open` (the default) or `closed`. `severity` is `low`, `normal`
(the default), or `high`; work through open issues by severity. CHECK
constraints reject other values. `details` is markdown with `## Problem`, `## Reproduction` (when there
is one), and `## Suggested fix` sections. `resolution` is markdown explaining
how a closed issue was addressed: the fixing commit, or why it will not be
fixed. A CHECK constraint rejects closing an issue without one. `issues_fts` is
an FTS5 index over title, details, and resolution, kept in sync by triggers;
never write to it directly. The database
uses WAL mode for the shared file; SQLite serializes writers. Set a busy timeout
so short competing writes wait instead of failing, and retry/reconcile explicitly
if the timeout is exhausted. WAL does not coordinate independent database copies.

| Task | Command |
|---|---|
| List open | `sqlite3 -readonly "$PLENTY_ISSUES_DB" "SELECT number, severity, title FROM issues WHERE status = 'open' ORDER BY CASE severity WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END, number"` |
| Read | `sqlite3 -readonly "$PLENTY_ISSUES_DB" "SELECT details FROM issues WHERE number = 3"` |
| Search | `sqlite3 -readonly "$PLENTY_ISSUES_DB" "SELECT i.number, i.status, i.title FROM issues_fts JOIN issues i ON i.number = issues_fts.rowid WHERE issues_fts MATCH 'stack overflow' ORDER BY rank"` |
| Add | In a short write transaction, repeat the duplicate search, then use a parameterized `INSERT INTO issues(title, severity, details) VALUES (?, ?, ?)` and read the assigned ID. |

Markdown with quotes is easier to insert through a parameterized query, for
example Python's `sqlite3` module, than through shell-quoted SQL. Search before
adding to avoid duplicates. Never delete an issue. To close one, set `status`
to `closed` and `resolution` in the same statement; leave `details` as the
record of the problem.

```sh
sqlite3 -cmd ".timeout 5000" "$PLENTY_ISSUES_DB" "UPDATE issues SET status = 'closed',
  resolution = 'Fixed in <commit>.' WHERE number = 3"
```

## Retrospective hygiene

`SUGGESTED_SYSTEM_IMPROVEMENTS.md` accumulates per-turn friction notes. Entries
can become stale or be superseded. When later work overturns a suggestion, add
`> **Superseded YYYY-MM-DD:** ...` under the original.
