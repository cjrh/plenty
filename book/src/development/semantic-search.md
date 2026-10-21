# Local semantic search trial

`examples/semantic_search.plenty` is a native Plenty CLI for a separate SQLite
search database. It uses `sqlite-vec` for exact cosine-distance search and
built-in FTS5 for keyword search, combining their top 30 candidates with
reciprocal rank fusion and returning up to ten chunks. All parsing, validation,
hashing, database operations, transactions and ranking are driven by Plenty.
Only model inference runs in Python, through `scripts/search_embed.py` and
FastEmbed. Plenty owns the CLI, chunking and document lifecycle.

This is a storage/interface experiment, not yet a repository-wide indexer. It
does not yet scan the repository, extract docstrings, read the issue register,
or discover deleted files. Remaining work belongs in
[the backlog](../backlog.md). No generated database or model is committed.

## Setup

Use the compiler built from this checkout and the pinned `sqlite-vec 0.1.9`
shared library for x86-64 Linux. One way to obtain the extension is its Python
wheel. Install FastEmbed in the same environment:

```sh
uv venv /tmp/plenty-search-env
uv pip install --python /tmp/plenty-search-env/bin/python sqlite-vec==0.1.9 fastembed==0.9.0
export PLENTY_SQLITE_VEC="$(/tmp/plenty-search-env/bin/python -c 'import sqlite_vec; print(sqlite_vec.loadable_path())')"
cargo run --locked --bin plenty -- --compile examples/semantic_search.plenty -o /tmp/plenty-search
```

The executable loads that library through
[`Connection.load_extension`](../design/stdlib/sqlite.md#native-extensions-and-full-text-search).
Use only a trusted extension. SQLite's SQL loading function remains disabled.
Storage-only commands do not need a Python process.

## Search document text

Run these commands from the checkout root. Setup downloads the pinned
`BAAI/bge-small-en-v1.5` quantized ONNX model (about 67 MB); inference thereafter
uses only the local cache and CPU. The helper pins the model artifact revision,
uses 384-dimensional vectors, and refuses input beyond its 512-token limit.

```sh
export SEARCH_PYTHON=/tmp/plenty-search-env/bin/python
export SEARCH_CACHE=/tmp/plenty-search-model-cache
"$SEARCH_PYTHON" scripts/search_embed.py --setup "$SEARCH_CACHE"
export SEARCH_MODEL="$("$SEARCH_PYTHON" scripts/search_embed.py --identity)"
/tmp/plenty-search "$PLENTY_SQLITE_VEC" /tmp/search.db "$SEARCH_MODEL" init 384
/tmp/plenty-search "$PLENTY_SQLITE_VEC" /tmp/search.db "$SEARCH_MODEL" index docs:sqlite SQLite book/src/design/stdlib/sqlite.md "$SEARCH_PYTHON" scripts/search_embed.py "$SEARCH_CACHE"
/tmp/plenty-search "$PLENTY_SQLITE_VEC" /tmp/search.db "$SEARCH_MODEL" find "How do I undo changes when a transaction fails?" "$SEARCH_PYTHON" scripts/search_embed.py "$SEARCH_CACHE"
```

`index ID TITLE TEXT_FILE PYTHON SCRIPT CACHE` reads one UTF-8 text document.
Plenty hashes its title and text before inference; unchanged input returns
immediately. Changed documents are split into 400-character windows with
64-character overlap, then embedded in one subprocess invocation. This simple
trial chunker can split sentences and Markdown structures; corpus-aware
chunking remains follow-on work. Empty files keep a document with zero chunks.
All replacement happens in one transaction after successful inference; a failed
subprocess, invalid response or wrong vector dimension leaves the old document.

`find QUERY PYTHON SCRIPT CACHE` embeds the query and runs hybrid search. The
process receives JSON text and returns JSON vectors; it never opens a database
or reads source files. There is no shell interpolation of paths or queries.
The helper reports errors on stderr, which Plenty preserves in its error.
Each indexing/query invocation loads the model anew, trading latency for a
simple setup without a background service.

Use `show ID`, `status` and `delete ID` with the same extension, database and
model arguments. Reindex after editing a file; explicitly `delete` its stable
ID after deleting it. Nothing modifies the original source or `issues.db`.

## Vector input and document operations

Every invocation starts with `EXTENSION DB MODEL`. Use a new database path,
separate from the authoritative `issues.db`. `MODEL` identifies the exact model,
revision, normalization and chunking configuration; changing it requires a new
database. The CLI rejects a different identity or initialization dimension.

The following example uses synthetic two-dimensional vectors to test storage;
they are not embeddings of the example text.

```sh
/tmp/plenty-search "$PLENTY_SQLITE_VEC" /tmp/search-trial.db fixture-v1 init 2
```

Save this JSON as `/tmp/search-chunks.json`:

```json
[
  {"text": "Owned resources are released on scope exit.", "embedding": [1.0, 0.0]},
  {"text": "Borrows cannot outlive their owners.", "embedding": [0.9, 0.1]}
]
```

```sh
/tmp/plenty-search "$PLENTY_SQLITE_VEC" /tmp/search-trial.db fixture-v1 put docs:ownership "Ownership" /tmp/search-chunks.json
/tmp/plenty-search "$PLENTY_SQLITE_VEC" /tmp/search-trial.db fixture-v1 show docs:ownership
/tmp/plenty-search "$PLENTY_SQLITE_VEC" /tmp/search-trial.db fixture-v1 status
```

`put` replaces all chunks belonging to that document in one transaction. Its
hash covers the title, chunk text and supplied vectors. Identical input prints
`unchanged`. Parsing happens before the transaction; if insertion fails (for
example, a vector has the wrong dimension), the previous document remains.
An empty chunk array keeps the document with no searchable chunks.

Save `[1.0, 0.0]` as `/tmp/search-query.json`, then query:

```sh
/tmp/plenty-search "$PLENTY_SQLITE_VEC" /tmp/search-trial.db fixture-v1 query /tmp/search-query.json "resource cleanup"
/tmp/plenty-search "$PLENTY_SQLITE_VEC" /tmp/search-trial.db fixture-v1 delete docs:ownership
```

Query words are quoted individually and joined with OR for FTS5. Empty words
give vector-only results. Results show document ID/title, zero-based chunk
number, fused score and text. Vectors must come from the same model and
preprocessing as the stored documents; the CLI cannot infer this from numbers.
Supply finite, nonzero vectors suitable for cosine similarity.

`delete` removes the document, its chunks and embedded vectors through a foreign
key cascade; triggers remove the associated FTS5 entries. It never deletes a
source file. The trial uses ordinary SQLite BLOB columns and `sqlite-vec`
distance functions, so vector ownership follows normal relational deletion.

## Validation

The deterministic integration test uses the actual extension and fake inference
and checks
insertion, unchanged input, search, rollback on malformed dimensions,
replacement, deletion, Unicode chunking, skipped inference and subprocess
failure preserving the previous document:

```sh
PLENTY_SQLITE_VEC="$PLENTY_SQLITE_VEC" cargo test --locked --test integration test_std_sqlite::sqlite_vec_search_trial -- --ignored
```

The test is explicitly ignored in the default suite because the extension is
an optional external dependency. The ordinary SQLite tests exercise extension
loading with a locally compiled fixture and check FTS5 without any download.
