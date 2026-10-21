# Search the repository

The local semantic index covers tracked Markdown documentation, source
documentation, and all local issue/PR titles and descriptions. It is a separate,
rebuildable database; `issues.db` remains authoritative. The CLI, extraction,
chunking, invalidation and queries run in Plenty. Python/FastEmbed performs only
embedding inference. See [the search interface](semantic-search.md) for the
model identity, storage schema and individual-document commands.

## Setup and use

From a checkout, install the pinned dependencies/model and build the executable:

```sh
bash scripts/search.sh setup
bash scripts/search.sh sync
bash scripts/search.sh find 'How do borrowed values interact with worker threads?'
bash scripts/search.sh status
bash scripts/search.sh check
bash scripts/search.sh show repo:doc:book/src/design/stdlib/sqlite.md
```

Setup needs `uv`, Cargo and a C linker, plus network access for the initial
packages and model. Subsequent indexing/search uses the local model. `build`
rebuilds just the executable after changing the Plenty source. The wrapper uses
`.search/` in the primary worktree for `search.db`, `semantic-search`, `venv/`
and `models/`. These generated files are ignored by Git. Model weights and the
environment are never part of an index snapshot.

The default corpus is the primary checkout, even when running the wrapper from
a task worktree. Set `PLENTY_SEARCH_ROOT` to index another checkout, or
`PLENTY_SEARCH_HOME` to use a separate index/environment. The issues register
must always be the primary worktree's absolute `issues.db` path; the Plenty
program validates it against `git worktree list`.

## Coverage and lifecycle

`check` is a deterministic freshness audit. It recomputes SHA-256 hashes of each
document's title and extracted text, compares them with the hashes stored in
`documents`, and lists added, changed and deleted IDs. It opens the search
database read-only, never invokes inference and does not modify embeddings.
Exit status is zero when fresh and nonzero when stale or on an operational
error (the diagnostic distinguishes these). This lets scripts/agents check
freshness without relying on a model's judgment. Only indexed text matters:
changing implementation code without changing extracted documentation need
not invalidate its embeddings. A rename is a deletion plus an addition.

- `repo:doc:PATH`: tracked Markdown, including the book and root/project docs.
  Vendor trees and the duplicate `BACKLOG.md` alias are excluded.
- `repo:source:PATH`: Rust `///`/`//!` documentation with declaration context;
  Python and Plenty module/function/class triple-quoted docstrings; and Plenty
  module headers and comment blocks preceding declarations. Source is grouped
  by file so its embeddings can be replaced/deleted together. The lightweight
  lexical extractor skips strings/test fixtures; it is not a general AST parser.
  Rust block doc comments and `#[doc = ...]` attributes are not extracted.
- `repo:issue:N` and `repo:pr:N`: titles and descriptions from a read-only SQLite
  snapshot, across all statuses. Discussion comments and PR code/diffs are not
  read. Code snippets already contained in descriptions remain part of that text.

`sync` scans Git's tracked paths (including staged new files and working-copy
edits), skips missing tracked files, and reads issues/PRs in one transaction.
Untracked files are excluded. It compares each document's extracted title/text
hash before inference, groups changed documents, and embeds at most 128 chunks
per subprocess call. Each document replacement is transactional. Successful
batches survive an interruption and are reused on the next run.

Only after the whole scan and all embeddings succeed does synchronization
remove missing `repo:` documents. Explicitly indexed IDs outside that prefix
are preserved. It then compacts the database. Run one sync at a time. A failed
run does not prune missing documents, and any document whose replacement fails
retains its previous chunks. `sync_info` records the source checkout, its HEAD
and the last successful completion time; working-copy edits are also indexed,
so HEAD alone is not a content fingerprint.

Run `sync` after document edits/deletions or issue/PR updates. Search results can
be stale between runs. The current 400-character chunker can split paragraphs
and Markdown structures; search is a discovery aid, so read the original source
before relying on a result. A different model/chunker identity requires rebuilding
the database.

## Tests

After setup/build, run the extraction/synchronization tests against the real
SQLite extension and a deterministic fake inference process:

```sh
PLENTY_SEARCH_BIN="$PWD/.search/semantic-search" \
PLENTY_SQLITE_VEC="$(.search/venv/bin/python -c 'import sqlite_vec; print(sqlite_vec.loadable_path())')" \
python3 -B scripts/test_search_index.py
```

The tests cover extraction, skipped inference, changed docs/issues, deletion,
failed inference, stale-register rejection, manual document preservation,
read-only freshness auditing, foreign keys and FTS cleanup. They need no model download and skip unless both
environment variables are set.
