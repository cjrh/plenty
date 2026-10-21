# Plenty — orientation for coding agents

Plenty is a statically typed language with Python-shaped syntax, single
ownership with checked borrows, and native compilation through Cranelift.
Design goals: low memory use, no hidden allocation, fast compilation, and a
small, understandable language. There is no interpreter, REPL, or JIT.

This file is for fast orientation. The documentation is authoritative; also
follow `AGENTS.md`.

## Local worktrees and pull requests

Issues and pull requests are entirely local records in the shared `issues.db`.
Do not create GitHub issues/PRs, add GitHub URLs, or fetch/push as part of this
workflow. A change request authorizes committing the scoped changes locally and
creating/updating a local PR record. The user reviews and authorizes merging;
do not merge or rewrite a branch under review without explicit instruction.

1. Inspect local status and worktrees. For issue work, read its description,
   comments, and linked PRs, then claim it atomically using the procedure below
   **before starting implementation**. Do not start an issue claimed by another
   agent. Continue your assigned task using its existing claim/worktree/PR.
   Create a separate worktree and descriptive branch from the local target
   branch (normally `main`) for new work. Keep the main worktree out of
   implementation work. Never reset/stash another worker's changes or include
   unrelated changes in a commit. An unnumbered task still needs a worktree/PR,
   but does not require inventing an issue just to claim it.
2. Establish responsibility for components/files before running workers
   concurrently. If work depends on another PR, state its base/dependency
   explicitly. Avoid unnecessary edits to shared documentation/backlog sections.
   Run commands with an explicit task worktree directory; do not assume another
   worker's files, build outputs, ports, or temporary paths belong to this task.
3. Implement and validate the change with its tests and reference/tutorial
   updates. For Rust changes, format and run the applicable workspace tests and
   lint checks; use `cargo test --workspace --all-features` when checking the
   full suite, since the runtime is a separate workspace member and both crates
   gate allocation tests behind features. Documentation-only changes need
   relevant document/link/example checks, not an unrelated full compiler build.
4. Review and stage only the task's files, commit locally, and create/update its
   PR record using the issues-browser CLI below. Explain the problem and final
   behavior, cite applicable backlog items, and report actual validation and
   limitations in a Markdown description file. Link every relevant local issue
   through a fixing reference in the title or a structured issue comment.
5. Run relevant checks locally and address failures caused by the change.
   Report blockers or checks not run accurately. Resolve conflicts within the
   task worktree and rerun affected checks. Hand off the local PR number,
   source/target branches, and worktree path. Keep the claim, branch, and
   worktree available through review and revisions until the user instructs you
   to merge.
6. After completing work on an issue, **"merge the PR"** authorizes the full
   completion workflow without a separate cleanup instruction. Merge the branch
   into its local target, mark the local PR as merged, update the linked issue
   (close it with the actual fixing commit and resolution when resolved), and
   release the task's claim. Check the docs and `book/src/backlog.md` and update
   them if necessary. Remove the task worktree only after confirming its changes
   are preserved; preserve unrelated changes and other workers' worktrees.

The issues database is the deliberate exception to isolated working files: all
workers use the main worktree's database as described below. Do not introduce
another task-status file. Planned work/priorities remain in `book/src/backlog.md`;
issue ownership, discussion, and PR review state live in the database.

## Where things are

| Path | Contents |
|---|---|
| `book/src/design/` | Language reference: current behavior, limits, architecture |
| `book/src/design/toolchain/02-architecture/support.md` | What works today |
| `book/src/design/stdlib/` | Standard library reference: one page per `std` module |
| `book/src/design/stdlib/signatures.md` | Generated index of every public `std` signature; search it before guessing an API |
| `book/src/tutorial/` | Learn Plenty: runnable lessons, checked by tests |
| `book/src/design/examples/cheatsheet.md` | Syntax cheat sheet: read before writing Plenty |
| `book/src/backlog.md` | The only list of future work and priorities |
| `book/src/proposals/` | Design rationale and alternatives (may contain old syntax) |
| `src/` | Compiler: frontend (parse, check) and Cranelift codegen |
| `plenty-runtime/` | Rust runtime crate, embedded in the compiler and linked into programs |
| `stdlib/std/` | Standard library modules in Plenty, embedded in the compiler |
| `tests/` | Integration tests, one area per file |
| `examples/` | Small sample programs and a compile-time benchmark |

`book/src/SUMMARY.md` lists every book page in reading order.

## Build & test

| Command | Use |
|---|---|
| `cargo test --locked --workspace` | Run compiler and runtime tests, without the allocation-accounting tests |
| `cargo test --locked --workspace --all-features` | Full suite, as CI runs it: adds the `runtime-checks` and `allocation-checks` tests |
| `cargo test --locked --test integration test_tutorial::` | Check the tutorial lessons |
| `PLENTY_CACHE=off cargo test ...` | Run without the compiled-function cache; `.cargo/config.toml` points it at `target/plenty-cache` for tests and `cargo run` |
| `cargo test --locked --test integration test_std_url::` | Run one test file's tests: integration tests are modules of `tests/integration.rs`, which must list each `tests/test_*.rs` file |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Lint compiler and runtime; CI's `lint` job also runs this, `cargo fmt --all -- --check`, and the link check below |
| `cargo run --locked --release --example compile_bench -- 1000 10 --workload aot` | Benchmark one workload; see `book/src/development/profiling.md` for workloads and phase profiles |
| `cargo run --bin plenty -- FILE` | Compile and run a Plenty program (needs `cc`); `--bin` is required because `src/bin/plentyc.rs` is a second binary |
| `cargo run --bin plenty -- --check FILE` | Type-check without linking or running |
| `cargo run --bin plenty -- --compile FILE -o OUT` | Produce a native executable |
| `target/release/plenty FILE` | Run probes with a compiler already built by `cargo build --release` in this worktree; rebuild after compiler/runtime changes |
| `python3 -B scripts/check_links.py book/src [SUBDIR_OR_FILE]` | Check local Markdown link targets across the book, or only e.g. `proposals`; no remote or heading-anchor checks |
| `python3 -B -m unittest discover -s scripts -p 'test_*.py'` | Check shared workflow helpers |
| `mdbook serve book --open` | Read the docs in a browser |

Running with no arguments prints all options.

The workspace `Cargo.lock` is tracked so worktrees use the same dependency
versions. Preserve it and use `--locked` for validation. Dependency updates are
deliberate changes using `cargo update`; include the resulting lockfile diff in
the PR. When Cranelift or regalloc2 changes, rerun object-identity checks and
affected performance comparisons. See the
[development workflow](book/src/development/workflow.md#dependency-versions).

Use explicit worktree directories for commands. Shell directory changes and
environment variables may not persist between tool calls. For summarized test
output, use the available output-filtering tools or capture a log while retaining
the original process exit status; a successful `grep` does not prove tests passed.

### Shell conditions and pipelines

Agent shells run with `set -o pipefail`, and an `rtk` hook rewrites some
commands before they run: `grep` becomes `rtk grep`, and a leading `git log` or
`cargo test` becomes `rtk git log` or `rtk cargo test`. `rtk grep` reads stdin
and keeps grep's exit status. These cause failures:

- `grep -q` or `grep -m1` stops reading at the first match, so the writer dies
  of SIGPIPE and the pipeline exits 141. `until git log main | grep -q X` never
  ends, even after `X` lands.
- `rtk git log` prints at most 50 commits. `rtk cargo test` replaces the
  `test result:` lines with one summary line.

In `if`, `until`, `while`, and `&&` conditions, prefer a command that needs no
pipe. In a pipe, use `grep -c` (reads all input), not `-q` or `-m`. Do not parse
`rtk` output; use its exit status. To wait for a merge, test the branch while it
exists and has commits; otherwise test the merge commit's subject:

```sh
until git -C "$PLENTY_MAIN_WORKTREE" merge-base --is-ancestor claude/issue-15 main; do sleep 30; done
until test -n "$(git -C "$PLENTY_MAIN_WORKTREE" log -1 --fixed-strings \
    --grep='Merge local PR #447' --format=%H main)"; do sleep 30; done
```

### Quick reproductions

Before writing Plenty, read the executable
[syntax cheat sheet](book/src/design/examples/cheatsheet.md); it lists the
Python habits the compiler rejects. Check standard library APIs in the generated
[signature index](book/src/design/stdlib/signatures.md). Executable statements belong in `main`; a bare top-level snippet is not a program.
For probes involving printing or allocation, use a fallible entry point:

```text
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    print(xs)?
    Ok(())
```

For an infallible body, use `def main() -> ():`. List/dict/set literals return a
`Result` and need handling, as does `print`. The test helper in
`tests/support/mod.rs` wraps fragments for fixtures; the public compiler expects
a complete program. See [the shared development workflow](book/src/development/workflow.md)
for documentation checks.

## Keeping things aligned

A learner-visible language change updates, in the same change:

- the implementation and its tests,
- the reference page(s) in `book/src/design/`,
- the tutorial lessons that teach the feature,
- the backlog entry, if its scope changed or it completed.

Tutorial and `design/examples/` code fences are executable tests, so stale
lessons fail the suite. Proposal fences are not executed; use `text` for
illustrative syntax and verify claims about implemented behavior separately.
Run the local link checker after documentation edits, especially moves.

### Tutorial maintenance

Follow the [tutorial guidelines in AGENTS.md](AGENTS.md#writing-and-maintaining-the-tutorial).
The tutorial teaches recommended ways to write Plenty today; the reference
describes implemented contracts and limits. Working examples can still become
outdated as the language grows, so executable checks do not replace an idiom
review.

When adding or changing a learner-visible feature:

1. Find existing lessons and larger examples that solve the affected problem,
   including manual workarounds and older library calls. Review them alongside
   the lesson introducing the feature and the relevant reference pages.
2. Ask whether each example is a good default for its task. Check error
   preservation and propagation, ownership and borrowing, unnecessary allocation
   or copying, and suitable standard-library operations. Prefer clarity and
   explicit costs over cosmetic brevity.
3. Check what the reader knows at that point in `SUMMARY.md`. Preserve a
   deliberate introductory loop or match; use later application examples to
   bring the taught conveniences together. Link forward when useful rather than
   introducing several unexplained concepts to shorten one example.
4. Distinguish a teaching simplification from a current implementation limit.
   Explain consequential differences from application code. For example,
   printing an error and returning success needs an intentional recovery or
   demonstration purpose; otherwise propagate it or return a failure status.
   Do not replace working code with syntax available only in a proposal.
5. Update affected examples, explanations, and reference links together. Lead
   with the recommended approach; move legacy details and exhaustive alternatives
   out of the main teaching path where practical. Avoid unrelated rewrites.

For changed executable examples, run
`cargo test --locked --test integration test_tutorial::`.
Keep `plenty-file` companions on the page that uses them, and retain checks for
expected output and diagnostics. Run
`python3 -B scripts/check_links.py book/src` after documentation edits; build the
book when changing page organization. Prose-only edits do not require rerunning
the compiler suite.

In the PR, explain which patterns improved and why, which teaching
simplifications or limitations remain intentional, and what was actually
validated. For a review sweep, state its coverage rather than implying that a
few sampled pages establish the quality of the whole tutorial.

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
export PLENTY_MAIN_WORKTREE="$(rtk proxy git worktree list --porcelain | sed -n '1s/^worktree //p')"
export PLENTY_ISSUES_DB="$PLENTY_MAIN_WORKTREE/issues.db"
export PLENTY_REVIEW_CLI="$(dirname "$PLENTY_MAIN_WORKTREE")/issues-browser/cli.js"
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
Never hold a transaction while researching, building, or waiting for review.

An open fixing PR is not a completed fix in the shared register. Keep the issue
open until the branch has actually been merged into the target branch under the
user's authorization; then record the PR as merged, close the fixed issue with
the actual fixing commit and
resolution, and release its claim. A PR status of `merged` is metadata, not proof
that Git has merged anything. Keep shared backlog status consistent with that
distinction. Closing an unmerged PR does not resolve the issue; its owner must
explicitly release the claim if abandoning the task.

Feature PRs must not stage `issues.db` from their task worktree: doing so can
replace newer shared entries with a stale branch snapshot. Persisting the tracked
register is a separate, coordinated maintenance change using a consistent SQLite
snapshot of the main database and the same PR review policy. Do not copy an active
WAL-mode database with an ordinary file copy. Do not overwrite/reset the live
database when switching/updating the main checkout; preserve newer rows and
reconcile logical changes transactionally. Database persistence must never
discard another worker's additions or edits.

### Claim an issue before starting work

Use a unique author label for each agent/task, for example
`codex-issue-15-<session-id>`. Read **all** comments and linked PRs before claiming:
an informal ownership comment or an existing open PR also requires coordination,
even if it predates the claim protocol. Never take over because a claim looks
old. Only resume another session's claim when assigned that task by the user.

The helper uses existing `comments` rows; it does not add tables or change issue
status. Run these from your checkout, always passing the shared database path:

```sh
rtk proxy sqlite3 -readonly "$PLENTY_ISSUES_DB" \
  "SELECT number, author, body, pull_request_number FROM comments WHERE issue_number = 15 ORDER BY number"
rtk proxy node "$PLENTY_REVIEW_CLI" --db "$PLENTY_ISSUES_DB" pr-list --status open
rtk proxy python3 scripts/issue_claim.py status --db "$PLENTY_ISSUES_DB" --issue 15
rtk proxy python3 scripts/issue_claim.py claim --db "$PLENTY_ISSUES_DB" --issue 15 \
  --author codex-issue-15-SESSION --note 'Implementing issue #15 in branch codex/issue-15, worktree /tmp/plenty-issue-15.'
```

**Proceed only if the claim command succeeds.** It uses `BEGIN IMMEDIATE` and a
five-second busy timeout to read active claims and insert one comment atomically.
A competing claim, a closed/missing issue, or a database error exits nonzero;
do not treat an error or timeout as permission to work. Keep the returned comment
number as the claim ID. Continuing your own assigned task reuses that claim;
calling `claim` again deliberately fails even for the same author.

A claim comment starts with the exact line `[claim]`. A release starts with
`[release #N]`, where `N` is the claim comment number, and has the same author.
Other comments, including PR links, do not release claims. Use the helper, not
a separate read followed by the ordinary `comment` command. These labels are a
cooperative ownership protocol, not authentication; never impersonate an owner.

Keep ownership through review and fixes. After completion or an explicit
abandonment/handoff, the owner releases it with a reason (replace `42` with the
actual claim comment number):

```sh
rtk proxy python3 scripts/issue_claim.py release --db "$PLENTY_ISSUES_DB" --issue 15 \
  --author codex-issue-15-SESSION --claim 42 --note 'Merged into main as COMMIT; issue resolved.'
```

For a user-authorized takeover, record the authorization in the release note
under the original owner label, then claim with the new owner's label. Do not
delete/edit old claims or silently release another agent's work. Claim helper
checks run with `rtk proxy python3 -B -m unittest discover -s scripts -p 'test_issue_claim.py'`.

### Create and link a local PR

The sibling Electron app `../issues-browser` supplies `cli.js` (Node.js 24+).
Resolve its path relative to the **primary** worktree as above, not relative to
a task worktree. If the CLI is missing, report the missing dependency; do not
fall back to GitHub. On an older database, run its additive migration before
using claims or PRs:

```sh
rtk proxy node "$PLENTY_REVIEW_CLI" --db "$PLENTY_ISSUES_DB" migrate
rtk proxy git worktree add -b codex/issue-15 /tmp/plenty-issue-15 main
```

Use task-specific branch/path values; reuse an assigned worktree. After working,
validating, and committing there, write the review description to a real Markdown
file and create the local PR (example paths and numbers must be replaced):

```sh
rtk proxy node "$PLENTY_REVIEW_CLI" --db "$PLENTY_ISSUES_DB" pr-create \
  --repo "$PLENTY_MAIN_WORKTREE" --worktree /tmp/plenty-issue-15 --target main \
  --title 'Fixes #15: explain the resulting behavior' --details-file /tmp/issue-15-pr.md
```

This inserts a `pull_requests` row with a local PR number, the actual source
branch, repository/worktree paths, target branch, title, and description. It
requires an existing registered worktree on a named branch and an existing
target branch; it does not create branches/worktrees or merge them. The GUI's
Pull Requests tab displays these records and the local branch comparison.

`Fixes #15`, `Closes #15`, and `Resolves issue #15` in the **title** create automatic
links to local issues. Alternatively (or additionally), write an issue comment
with the PR foreign key; a number only in the comment text is insufficient:

```sh
rtk proxy node "$PLENTY_REVIEW_CLI" --db "$PLENTY_ISSUES_DB" comment \
  --issue 15 --pr 3 --author codex-issue-15-SESSION --body-file /tmp/issue-15-review-comment.md
rtk proxy node "$PLENTY_REVIEW_CLI" --db "$PLENTY_ISSUES_DB" pr-show --number 3
```

Reuse the same record for revisions. Read it first and pass its returned
`revision` to detect concurrent edits (here `1` is only an example):

```sh
rtk proxy node "$PLENTY_REVIEW_CLI" --db "$PLENTY_ISSUES_DB" pr-update \
  --number 3 --revision 1 --details-file /tmp/issue-15-pr.md
```

If the revision is stale, reread and reconcile instead of overwriting another
worker's changes. PR statuses are `open`, `merged`, and `closed`; there is no
draft status. Describe incomplete work/checks explicitly. Once the user has
authorized merging and the branch has actually been merged, use
`pr-update --number N --revision R --status merged` with current values, close
the fixed issue with a resolution, and release the
claim. Neither automatic links nor status updates close issues or perform Git
operations. Keep the branch/worktree available for review and revisions until
the user instructs you to merge; that instruction includes the issue updates,
necessary docs/backlog updates, and worktree cleanup described above.

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

The issues-browser migration also maintains:

- `comments(number, issue_number, pull_request_number, author, body, created_at)`:
  `issue_number` is a required foreign key; `pull_request_number` is an optional
  foreign key. Claims, releases, discussion, and PR links are append-only comments.
- `pull_requests(number, title, details, repository_path, worktree_path,
  source_branch, target_branch, status, created_at, updated_at, revision)`:
  one open PR per repository/source/target combination. Updates increment
  `revision`; use the CLI for validation and optimistic concurrency checks.

Enable `PRAGMA foreign_keys = ON` on every connection writing comments/PRs.
Use the CLI's migration; do not invent competing table layouts. Title-derived
issue links are computed by the app/CLI, not stored in a separate link table.

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

FTS searches cover title, details, and resolution. To narrow noisy results, use
`issues_fts MATCH 'title : "verifier"'` with the same join as the search example.
Also search the full text before adding an issue: a title-only search can miss
an existing report with different wording.

```sh
sqlite3 -cmd ".timeout 5000" "$PLENTY_ISSUES_DB" "UPDATE issues SET status = 'closed',
  resolution = 'Fixed in <commit>.' WHERE number = 3"
```

## Retrospective hygiene

`SUGGESTED_SYSTEM_IMPROVEMENTS.md` accumulates per-turn friction notes. Entries
can become stale or be superseded. When later work overturns a suggestion, add
`> **Superseded YYYY-MM-DD:** ...` under the original.
