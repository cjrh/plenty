# std.db.sqlite

`import std.db.sqlite` stores and queries data in SQLite databases through
prepared statements with typed parameters and typed column reads. It has one
error type, `DbError`.

For named byte-key maps without application SQL, use [`std.db.kv`](kv.md),
which shares this engine. Use this module for relational schemas, SQL queries,
or reusable prepared statements. See the [storage comparison](kv-engine.md).

SQLite is vendored C code compiled into the runtime. The Plenty module owns
the error model, row state, value limits, and transaction ownership. It is
not an ORM: SQL is written by the program, and values are bound, never
spliced into SQL text. The module is available on the supported x86-64 Linux
GNU target.

## Opening

| Function | Result | Contract |
|---|---|---|
| `connect(path: &str, mode: Mode, max_length: i64)` | `Result[Connection, DbError]` | The database file at `path` |
| `memory(max_length: i64)` | `Result[Connection, DbError]` | A new private database in memory, gone when the connection closes |

`Mode` is `ReadOnly`, `ReadWrite` (the file must exist), or `Create`. A
missing file without `Create` is SQLite's `cant_open` (14). `path` is a file
name, not a URI; `""`, `":memory:"`, and paths containing a NUL byte are
`InvalidPath`.

`max_length` is the largest text, blob, or row, in bytes, that the
connection stores or reads. It must lie in `min_length_limit` (1024) to
`max_length_limit` (1,000,000,000, SQLite's own maximum); otherwise
`OutOfRange`. Binding, computing, or reading a larger value is `TooLarge`.
Choose it from the memory the program can spend: every text or blob read is
copied into a new `str` or `Bytes`.

## Connection

| Method | Result | Contract |
|---|---|---|
| `execute(sql: &str)` | `Result[(), DbError]` | Run every statement in `sql` to completion, discarding rows |
| `prepare(sql: &str)` | `Result[Statement, DbError]` | Compile exactly one statement |
| `begin(mode: Begin)` | `Result[Transaction, DbError]` | Start a transaction |
| `changes()` | `i64` | Rows changed by the most recent INSERT, UPDATE, or DELETE |
| `last_insert_rowid()` | `i64` | The rowid of the most recent successful INSERT |
| `set_busy_timeout(milliseconds: i64)` | `Result[(), DbError]` | How long to retry a locked database; see [Busy databases](#busy-databases) |
| `load_extension(path: &str, entrypoint: &str = "")` | `Result[(), DbError]` | Load a trusted native SQLite extension into this connection |
| `close()` | `Result[(), DbError]` | Close now |

`execute` takes no parameters and is meant for schema and fixed SQL.
`prepare` rejects SQL with no statement (`EmptySql`) or more than one
(`TrailingSql`); a trailing `;`, whitespace, or comment is allowed.

`close` is `InUse` while a statement or transaction made from the
connection is alive, and leaves it open. After a successful `close`, every
method that reaches SQLite is `Closed`, `changes` and `last_insert_rowid`
return 0, and a second `close` succeeds. Dropping a connection without
`close` closes it once its last statement and transaction are gone.

## Native extensions and full-text search

`load_extension` loads a native shared library for the supported target. Only
load libraries you trust: their initialization and SQL functions execute native
code in your process. The extension must use SQLite's extension API table;
it does not need a separately linked SQLite library. Use an explicit filesystem
path to select the library. SQLite may try platform suffixes if it cannot open
that path directly.

An empty `entrypoint` asks SQLite to discover the initialization symbol;
otherwise it names the exact exported symbol (for example, `sqlite3_vec_init`).
Empty or NUL-containing paths are `InvalidPath`; a NUL in the entrypoint is
`InvalidExtensionEntry`. Loader failures return `Sqlite` with SQLite's loader
message. `Closed` is returned if the connection has been closed.

The native loading API is enabled only during the call, under the connection
mutex, and disabled again on success or failure. SQL's `load_extension()` stays
disabled. Extensions belong to the connection and must be loaded again when
reopening a database that uses them. There is no public enable/disable toggle.

FTS5 is built in: `CREATE VIRTUAL TABLE ... USING fts5(...)`, `MATCH`, and
`bm25(...)` work without loading an extension. Vector extensions such as
`sqlite-vec` are installed separately; Plenty does not download them.

## Statement

Parameters are numbered from 1 and columns from 0.

| Method | Result | Contract |
|---|---|---|
| `parameter_count()` | `i64` | The largest parameter number |
| `parameter_index(name: &str)` | `Result[i64, DbError]` | The number of `:name`, `@name`, or `$name`, prefix included; unknown is `OutOfRange` |
| `bind_null(index)` | `Result[(), DbError]` | |
| `bind_integer(index, value: i64)` | `Result[(), DbError]` | |
| `bind_real(index, value: f64)` | `Result[(), DbError]` | NaN is `OutOfRange` |
| `bind_text(index, value: &str)` | `Result[(), DbError]` | Copied |
| `bind_blob(index, value: &Bytes)` | `Result[(), DbError]` | Copied |
| `clear_bindings()` | `Result[(), DbError]` | Every parameter back to NULL |
| `step()` | `Result[bool, DbError]` | Advance: `True` with a row current, `False` once finished |
| `reset()` | `()` | Rewind, keeping the bindings |
| `run()` | `Result[(), DbError]` | Reset, step to the end discarding rows, reset again |
| `column_count()` | `i64` | Columns in each row |
| `column_name(column)` | `Result[str, DbError]` | The column's name or `AS` alias |
| `kind(column)` | `Result[ValueType, DbError]` | The storage class of the value in the current row |
| `is_null(column)` | `Result[bool, DbError]` | |
| `integer(column)`, `real(column)`, `text(column)`, `blob(column)` | `Result[i64 / f64 / str / Bytes, DbError]` | The value, which must have that storage class |
| `optional_integer(column)` and the other `optional_` reads | `Result[Option[...], DbError]` | `Nothing` for NULL, otherwise as the strict read |

An index outside the statement's parameters or columns is `OutOfRange`.
Binding after a step rewinds the statement first, so a statement is reused
by binding new values and stepping again. A statement that has finished
keeps returning `False` from `step` until `reset` or a new binding. A
failed step rewinds it.

Reads require a current row (`NoRow` otherwise) and never convert: SQLite's
storage class must match the read, or the error is
`TypeMismatch(found)`, where `found` is the stored class. `ValueType` is
`Integer`, `Real`, `Text`, `Blob`, or `Null`. A column's declared type does
not decide its storage class; SQLite's type affinity does, so an `INTEGER`
column given the text `'7'` stores the integer 7, but a `TEXT` column given
the integer 7 stores the text `'7'`. `STRICT` tables still allow lossless
conversion, including `'7'` into an `INTEGER` column; they reject values that
cannot be losslessly converted to the declared type. Text that is not valid
UTF-8 is `InvalidUtf8`.

Every text and blob read is copied out, so values stay valid after the next
`step`, `reset`, or the statement's drop. There are no borrowed row views.

## Transactions

`Begin` is `Deferred` (locks taken on first use), `Immediate` (write lock at
once), or `Exclusive`.

| Method | Result | Contract |
|---|---|---|
| `commit()` | `Result[(), DbError]` | Make the changes durable |
| `rollback()` | `Result[(), DbError]` | Discard the changes |

A connection has at most one transaction: `begin` while one is open is
`TransactionActive`. Dropping a transaction that was neither committed nor
rolled back rolls it back, so `?` out of a function undoes its work.
A finished transaction, or one ended by SQL such as `COMMIT` run through
`execute`, is `TransactionFinished` on `commit` and `rollback`. A failed
`commit` that SQLite leaves open, such as one that is `busy`, may be retried
or rolled back.

Savepoints are available through `execute("SAVEPOINT name")` inside a
transaction.

## Busy databases

A write needs a lock on the database file. When another connection holds
it, the default is to fail at once with SQLite's `busy` (5) code.
`set_busy_timeout(ms)` makes later operations retry for up to `ms`
milliseconds first; 0 restores the default. Values outside 0 to
2,147,483,647 are `OutOfRange`. Use `Begin.Immediate` for transactions that
write, so a conflict is found at `begin` rather than partway through.

## Errors

```text
pub enum DbError:
    Sqlite(SqliteError)
    Closed
    InUse
    EmptySql
    TrailingSql
    TransactionActive
    TransactionFinished
    InvalidPath
    InvalidExtensionEntry
    OutOfRange
    NoRow
    TypeMismatch(ValueType)
    TooLarge
    InvalidUtf8
    Allocation(AllocError)
```

| Variant | Meaning |
|---|---|
| `Sqlite(error)` | A failure reported by SQLite: `error.code` is its extended result code, `error.message` its message |
| `Closed` | The connection was closed |
| `InUse` | `close` with statements or a transaction alive |
| `EmptySql`, `TrailingSql` | `prepare` given no statement or more than one |
| `TransactionActive` | `begin` with a transaction already open |
| `TransactionFinished` | `commit` or `rollback` on a finished transaction |
| `InvalidPath` | An empty, `":memory:"`, or NUL-containing path |
| `InvalidExtensionEntry` | An extension entrypoint containing NUL |
| `OutOfRange` | A parameter, column, limit, or timeout out of range, a NaN, or an unknown parameter name |
| `NoRow` | A read with no current row |
| `TypeMismatch(found)` | A read of the wrong storage class |
| `TooLarge` | A value or row over the connection's `max_length` |
| `InvalidUtf8` | Stored text that is not UTF-8 |
| `Allocation(cause)` | The runtime or SQLite could not allocate |

`SqliteError.primary()` is the primary result code, the extended code modulo
256. The module names the common ones as constants: `error` (1), `busy` (5),
`locked` (6), `read_only` (8), `cant_open` (14), `constraint` (19), and
`mismatch` (20). For example, a NOT NULL violation has code 1299 and primary
code 19. SQLite's own `nomem`, `toobig`, and `range` codes become
`Allocation`, `TooLarge`, and `OutOfRange`.

## Ownership and threads

A `Connection`, `Statement`, or `Transaction` is an owned value; none can be
copied. A statement or transaction keeps its connection's SQLite handle
alive, so a statement may outlive the `Connection` value it came from.
Dropping a statement finalizes it.

SQLite is compiled thread-safe and every connection is opened in serialized
mode, so a connection, statement, or transaction may be moved to a worker
thread and used there. Opening and loading an extension are path effects;
other operations act on a handle the caller owns. Extension code must itself
support the threads on which its functions are used. SQLite's messages are copied per thread, so
concurrent errors on different threads do not overwrite each other.

The handles are `ByteStorage` values. The `__sqlite_*` primitives and the
`sqlite_bind` and `sqlite_column` helpers in `std.bytes` exist only for this
module. Given a handle not made by it, they stop the program with
`operand is not a std.db.sqlite connection` (or `statement`, or
`transaction`).

## Costs

| Operation | Allocates |
|---|---|
| `connect`, `memory` | A copy of the path, the connection owner, and SQLite's connection state and page cache |
| `load_extension` | Copies of the path and entrypoint, plus loader and extension allocations |
| `prepare`, `begin` | The statement or transaction owner and SQLite's compiled program |
| `bind_*`, `step`, `reset`, `kind`, `integer`, `real` | Nothing in the runtime; SQLite copies bound text and blobs |
| `text`, `blob`, `column_name` | The returned value |

SQLite allocates through the C allocator, outside Plenty's allocation
accounting and failure injection; its own allocation failures are reported
as `Allocation`.

## Supply

The runtime compiles the SQLite amalgamation from an unmodified copy under
`plenty-runtime/native/sqlite-3.54.0`, so building the compiler needs a C
compiler (`cc`, or `CC`) but no network, and an installed compiler needs
neither the sources nor a system SQLite:

| Library | Version | Licence | Source |
|---|---|---|---|
| SQLite | 3.54.0 | Public domain | `sqlite-amalgamation-3540000.zip` from sqlite.org, SHA3-256 `7b670a62fdfbd672b75fef004cb703c8a3e87d3a5cc7d675b4a08337004a2d93` |

It is built with `SQLITE_THREADSAFE=1`, `SQLITE_DEFAULT_MEMSTATUS=0`,
`SQLITE_DQS=0` (double-quoted strings are identifiers only),
`SQLITE_LIKE_DOESNT_MATCH_BLOBS`, `SQLITE_OMIT_DEPRECATED`,
`SQLITE_ENABLE_FTS5`, and `SQLITE_OMIT_SHARED_CACHE`. Native extension loading
is available through `Connection.load_extension`; SQL loading stays disabled.

Updating it is the runtime maintainers' job, with each SQLite release that
fixes a defect: replace the directory with the new amalgamation, verify its
published SHA3-256, update the directory name in `build.rs` and the table
above, and rerun `tests/test_std_sqlite.rs`, which checks that the vendored
header, this page, and `sqlite_version()` agree.

Executables are linked with `--gc-sections`, SQLite is compiled with
function and data sections, and it is reached only through its own runtime
entry symbols, so a program that does not use `std.db.sqlite` contains none of it.
A program that uses SQLite includes the database engine and its enabled features.

## Limits

- No connection pool, ORM, or query builder.
- No user-defined SQL functions, collations, hooks, or virtual tables.
- No incremental blob I/O, backup API, or online serialization.
- No URI file names.
- No borrowed row views: every text and blob read is a copy.
- SQLite's allocations are outside Plenty's allocation accounting.
- One target, x86-64 Linux GNU.
