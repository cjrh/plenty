# Choosing key-value storage

`std.db.kv` is a convenience interface over SQLite, not a new database engine.
The implementation keeps byte-key policy in Plenty and reuses SQLite for
storage, transactions, locking and crash recovery.

| Choice | Best fit | Tradeoffs |
|---|---|---|
| `std.db.kv` | A dedicated file of named byte-key maps, ordered scans and atomic batches | Small API; copied byte values; prepares statements per call; only the documented KV schema |
| `std.db.sqlite` | SQL queries, relational tables, joins, indexes, shared application schemas, or tuned prepared-statement reuse | More application SQL, but access to the same mature engine and tools |
| redb (not shipped by Plenty) | An application wanting a specialized embedded ordered KV engine | Rust-native typed/zero-copy access and MVCC; another file format and dependency; its allocation contract currently prevents a thin Plenty wrapper |

SQLite implements ordered-key storage through
[`WITHOUT ROWID`](https://sqlite.org/withoutrowid.html). Our composite primary
key is `(table_name,key)`, so scans can seek to a namespace and byte range.
This avoids a separate rowid table plus primary-key index. SQL parsing,
binding and copied-out values still cost work. Direct SQLite can reuse prepared
statements for a hot loop; the initial KV convenience API does not cache them.
Batch transactions often matter more than individual call overhead for writes.
No benchmark here establishes that SQLite is faster or slower than redb for
your workload; durability settings, batches, cache and key sizes must match.

## Why not wrap redb now?

The evaluated crate is **redb 4.4.0**, downloaded for source inspection only;
it is not a build dependency. Its published design uses copy-on-write B+trees
with concurrent snapshot readers and a writer. Its README describes a stable
file format with reasonable efforts toward future upgrades. Those are useful
properties for a dedicated KV engine.

However, its ordinary database creation and page-cache paths contain
infallible `Box::new`, `Arc::new`, vector allocation and collection into
`Arc<[u8]>`. Concrete examples are `Builder::new`/database setup in `src/db.rs`
and `zero_filled_arc`/`read_direct` in
`src/tree_store/page_store/cached_file.rs`. There is no fallible-allocation
boundary around those paths that a small Plenty wrapper can use. Returning
`Result` for database I/O does not make those allocations fallible, and
catching a panic does not recover from allocator aborts.

See the [published crate source](https://docs.rs/crate/redb/4.4.0/source/)
and [crate description](https://crates.io/crates/redb/4.4.0).
Meeting Plenty's requirement would need a substantial fallible-allocation
audit/change to the engine, not just converting its public error enum.
SQLite already reports allocation failures via `SQLITE_NOMEM`, which Plenty's
existing adapter maps to `AllocError`. Reusing it is a much smaller change
than building and validating another durable storage engine in Plenty.

This choice gives Plenty users the requested KV ergonomics now. It does not
promise redb-compatible files, redb-level zero-copy reads, or backend-neutral
file interchange. The [KV reference](kv.md#file-compatibility) publishes the
actual SQLite schema and compatibility rules.
