# std.db.kv

`import std.db.kv` provides persistent, ordered byte-key storage without SQL
in application code. The small public API is implemented in Plenty over the
existing [`std.db.sqlite`](sqlite.md) engine. It adds no native dependency.
See [the engine decision](kv-engine.md) for the SQLite/redb comparison.

```plenty
import std.bytes as buffers
import std.db.kv as kv
from std.bytes import BytesError

def main() -> Result[(), IoError | BytesError | kv.KvError]:
    mut store = kv.connect("settings.db")?
    store.put("config", &buffers.from_text("theme")?, &buffers.from_text("dark")?)?
    store.close()?

    mut reopened = kv.connect("settings.db", mode=kv.Mode.ReadOnly)?
    match reopened.get("config", &buffers.from_text("theme")?)?:
        case Some(value):
            print(value.to_text()?)?
        case Nothing:
            print("unset")?
    print(reopened.len("config")?)?
    reopened.close()?
    Ok(())
```
```output
dark
1
```

## Stores and tables

`connect(path, mode=Mode.Create, max_length=1048576)` returns
`Result[Store, KvError]`. Modes are `Create`, `ReadWrite` (existing file) and
`ReadOnly`. `memory(max_length=1048576)` creates a private in-memory store.
Paths are UTF-8 strings with the same validation as SQLite paths.

Logical table names are nonempty strings, bound as data rather than interpolated
into SQL. Each table is an independent namespace. It appears on first `put`;
an empty table and a nonexistent table behave identically. Keys and values
are `std.bytes.Bytes`, including empty buffers, NULs and non-UTF-8 bytes.
Keys sort lexicographically by unsigned bytes, shorter equal prefixes first.
Integer encodings must be chosen deliberately if numerical ordering matters.

| Store method | Result | Behavior |
|---|---|---|
| `put(table, key, value)` | `Result[(), KvError]` | Insert or replace one key |
| `get(table, key)` | `Result[Option[Bytes], KvError]` | Copy out the value, or `Nothing` |
| `contains(table, key)` | `Result[bool, KvError]` | Test presence without copying the value |
| `remove(table, key)` | `Result[bool, KvError]` | True if an entry was removed |
| `clear(table)` | `Result[i64, KvError]` | Remove all entries; return their count |
| `len(table)` | `Result[i64, KvError]` | Count entries (a table scan, not a cached counter) |
| `set_busy_timeout(milliseconds)` | `Result[(), KvError]` | Retry lock conflicts for up to this timeout; default zero |
| `close()` | `Result[(), KvError]` | Close, or `Database(InUse)` while transactions/scans still own handles |

All methods take `&mut Store`. Key/value arguments are borrowed and copied
into SQLite; returned values are owned and survive the next operation. Ordinary
methods prepare a statement per call. Use transactions to amortize commit I/O.

## Transactions

`store.begin(immediate=True)` returns a `Transaction`. Its `commit()` and
`rollback()` return `Result[(), KvError]`. Dropping an unfinished transaction
rolls back, including when `?` exits a function. Every operation on that Store
participates until completion. Outside a transaction each write auto-commits.

Only one transaction may be active per Store. Immediate mode takes the write
lock at the beginning; `immediate=False` defers locks until first use and is
appropriate for a read transaction. Deferred does **not** prohibit writes;
use `Mode.ReadOnly` for a read-only connection. A busy commit can be retried
or rolled back according to SQLite's transaction contract.

Different Store connections follow SQLite isolation and locking. This module
uses SQLite's default rollback journal rather than enabling WAL automatically.
Readers may delay a writer's commit. No background thread, automatic retry
loop or connection pool is introduced.

## Bounded scans

`scan(table, start, stop, max_rows, after=False, reverse=False)` returns a
`Scan` over `[start, stop)`; `after=True` excludes `start` too, allowing an
ascending scan to resume after its last returned key. `reverse=True` changes
the order without changing the bounds. Reversed/equal bounds yield no rows.
`scan_all(table, max_rows, reverse=False)` has no key bounds, so it can include
every byte key, including keys composed entirely of `0xff` bytes.

The row limit is required: negative limits are rejected and zero yields
nothing. A limit truncates the scan, not an error; it does not promise that no
more matching rows exist. `Scan.next()` returns `Result[Option[Entry], KvError]`;
each `Entry` owns public `key` and `value` buffers. Only one row is copied per
call. A copy allocation failure preserves the pending row for retry; a SQLite
step failure terminates that scan, avoiding silent replay of earlier rows.
Repeated calls after exhaustion return `Nothing`.

The snapshot starts at the first `next`, not construction. Finish/drop scans
before writing through the same Store or ending its transaction: SQLite does
not promise a stable scan when its own connection changes the rows being read.
Separate Store connections supply normal SQLite snapshot isolation. A Scan
keeps its underlying connection alive even if the Store is dropped. Drop the
Scan to release its statement and any read lock promptly.

## Limits, errors and allocation

`max_length` is SQLite's per-text/blob/encoded-row limit, between 1024 and
1,000,000,000 bytes. It includes row encoding overhead; the maximum value
payload is therefore smaller. It is not a total database size, page-cache
memory cap or execution deadline. Callers choose scan row limits independently.

`KvError` includes `InvalidName`, `InvalidLimit`, `ForeignDatabase`,
`UnsupportedVersion(version)`, `Allocation(AllocError)` and
`Database(std.db.sqlite.DbError)`. SQLite allocation errors are flattened to
`KvError.Allocation`; other database failures retain their structured cause,
including busy, read-only, I/O, closed handles and overlarge values.

Plenty-owned handle/row allocations participate in allocation accounting and
failure injection. SQLite's C allocations are outside those hooks, but its
`SQLITE_NOMEM` failures are returned through the same allocation error.

## File compatibility

Files use SQLite's documented format and a versioned Plenty schema:

```sql
PRAGMA application_id = 1347177302; -- ASCII PLKV
PRAGMA user_version = 1;
CREATE TABLE plenty_kv(
    table_name TEXT NOT NULL,
    key BLOB NOT NULL,
    value BLOB NOT NULL,
    PRIMARY KEY(table_name,key)
) STRICT, WITHOUT ROWID;
```

`Mode.Create` initializes only an empty, untagged SQLite database, in one
transaction. It does not repurpose an unrelated database. Opening a known
application ID with an unsupported version fails without migrating it. Version
1 remains readable across Plenty releases; future incompatible changes require
an explicit migration/export path rather than silently rewriting old files.
SQLite tools can inspect a store directly; use SQLite 3.37+ for `STRICT` tables.
Manual schema/tag changes are unsupported, and the application ID is a format
tag, not authentication of an untrusted database.

Durability, recovery and journals remain SQLite's responsibility. Do not copy
an active database by copying just its main file: its journal may be needed
for recovery. This API does not yet supply online backup or compaction methods.
See [SQLite file format](https://sqlite.org/fileformat.html) and
[atomic commit](https://sqlite.org/atomiccommit.html).

The [tutorial](../../tutorial/08-resources/key-value-store.md) shows a batch
transaction and bounded scan. Direct SQL, alternate engines, prefix helpers,
zero-copy values and automatic typed serialization are outside this API.
