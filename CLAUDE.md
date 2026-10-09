# Plenty — orientation for coding agents

Plenty is a statically typed language with Python-shaped syntax, single
ownership with checked borrows, and native compilation through Cranelift.
Design goals: low memory use, no hidden allocation, fast compilation, and a
small, understandable language. There is no interpreter, REPL, or JIT.

This file is for fast orientation. The documentation is authoritative; also
follow `AGENTS.md`.

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
| `cargo test` | Run the full suite |
| `cargo test 2>&1 \| grep "test result"` | Reliable summary across all suites |
| `cargo test --test test_tutorial` | Check the tutorial lessons |
| `cargo clippy --all-targets -- -D warnings` | Lint |
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

Table `issues(number INTEGER PRIMARY KEY, title TEXT, details TEXT, status TEXT, severity TEXT, resolution TEXT)`.
`status` is `open` (the default) or `closed`. `severity` is `low`, `normal`
(the default), or `high`; work through open issues by severity. CHECK
constraints reject other values. `details` is markdown with `## Problem`, `## Reproduction` (when there
is one), and `## Suggested fix` sections. `resolution` is markdown explaining
how a closed issue was addressed: the fixing commit, or why it will not be
fixed. A CHECK constraint rejects closing an issue without one. `issues_fts` is
an FTS5 index over title, details, and resolution, kept in sync by triggers;
never write to it directly. The database
uses WAL mode, so concurrent readers and writers are safe; set a busy timeout
so writers wait instead of failing.

| Task | Command |
|---|---|
| List open | `sqlite3 issues.db "SELECT number, severity, title FROM issues WHERE status = 'open' ORDER BY CASE severity WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END, number"` |
| Read | `sqlite3 issues.db "SELECT details FROM issues WHERE number = 3"` |
| Search | `sqlite3 issues.db "SELECT i.number, i.status, i.title FROM issues_fts JOIN issues i ON i.number = issues_fts.rowid WHERE issues_fts MATCH 'stack overflow' ORDER BY rank"` |
| Add | `sqlite3 -cmd ".timeout 5000" issues.db "INSERT INTO issues(title, severity, details) VALUES ('...', 'normal', '...')"` |

Markdown with quotes is easier to insert through a parameterized query, for
example Python's `sqlite3` module, than through shell-quoted SQL. Search before
adding to avoid duplicates. Never delete an issue. To close one, set `status`
to `closed` and `resolution` in the same statement; leave `details` as the
record of the problem.

```sh
sqlite3 -cmd ".timeout 5000" issues.db "UPDATE issues SET status = 'closed',
  resolution = 'Fixed in <commit>.' WHERE number = 3"
```

## Retrospective hygiene

`SUGGESTED_SYSTEM_IMPROVEMENTS.md` accumulates per-turn friction notes. Entries
can become stale or be superseded. When later work overturns a suggestion, add
`> **Superseded YYYY-MM-DD:** ...` under the original.
