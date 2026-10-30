---
name: plenty-search
description: For work inside the Plenty compiler repository. Find related Plenty concepts across repository docs, source documentation, and local issue/PR descriptions using the local semantic index. Use for conceptual discovery or index freshness checks; use rg for exact symbols and implementation code. To write or fix Plenty code, use the plenty skill.
---

# Plenty semantic search

Run commands from a Plenty checkout, following its RTK command-prefix convention.
The wrapper uses the primary worktree's `.search/` and indexes the primary
checkout by default, even when invoked from a task worktree.

## Find and verify

Check freshness once at the start of a related search session, and again after
source/register changes that matter to the task:

```sh
rtk proxy bash scripts/search.sh check
rtk proxy bash scripts/search.sh find 'How do borrowed values interact with worker threads?'
rtk proxy bash scripts/search.sh show repo:doc:book/src/design/stdlib/sqlite.md
```

`check` compares stored SHA-256 hashes with current extracted source text. It
does not run inference or write the index. Exit zero means fresh; nonzero can
mean stale **or an operational error**. Read the diagnostic: stale output lists
added/changed/deleted IDs. When those changes affect the task or a refresh was
requested, run `sync`, then `check` again. Run only one sync at a time.

```sh
rtk proxy bash scripts/search.sh sync
rtk proxy bash scripts/search.sh status
```

Search with a short concept/question. If results are weak, try the domain terms
found in relevant hits or use `rg`; absence of a result is not proof of absence.
Results contain document IDs, titles, zero-based chunk numbers, scores and text.
Chunk numbers are not source line numbers, and scores are rankings, not confidence.

Read the original before drawing conclusions:

- `repo:doc:PATH` / `repo:source:PATH`: open the source path. In a task worktree,
  compare with the primary checkout when differences matter; the indexed text
  may describe a different revision from the file you are editing.
- `repo:issue:N` / `repo:pr:N`: read the local record through the shared issues
  CLI documented in [CLAUDE.md](../../../CLAUDE.md), including current status
  and relevant discussion. These are local numbers, not GitHub links.

Prefer current reference/implementation evidence over an old proposal or issue
description. Use `rg` for exact names, implementation bodies, untracked files,
and discussion/code outside the index's coverage.

## Missing setup and maintenance

If the environment, model or executable is missing during ordinary code search,
report that briefly and continue with `rg` and direct reads. Do not reinstall
dependencies or rebuild an otherwise working environment for each search.
If setting up or repairing semantic search is part of the task, follow the
[repository search guide](../../../book/src/development/semantic-search-corpus.md):
`setup` installs the pinned packages/model and builds; `build` only rebuilds the
executable; `sync` creates/refreshes the database. A missing database alone does
not require recreating the venv. Treat a model-identity mismatch as a configuration
problem, not permission to delete the existing index.

Keep the shared index pointed at the primary checkout. Use `rg` for task-worktree
edits; intentionally indexing another corpus requires a separate search home,
as described in the guide. Always retain the primary worktree's authoritative
`issues.db`; the search database is separate and derived. Generated state stays
local unless the user requests a committed snapshot.

The index covers tracked Markdown, supported source docstrings/comments, and
issue/PR titles and descriptions. It omits implementation bodies, issue comments,
PR diffs, vendor trees and untracked files. See the guide for extractor limits.
