# Store byte-keyed records

Use `std.db.kv` when a program needs named maps of byte keys and values, and
ordered scans, without writing SQL. Use `std.db.sqlite` when SQL queries,
joins or your own database schema are part of the application.

Batch related writes in a transaction. A failed operation propagated by `?`
drops the unfinished transaction and rolls the batch back:

```plenty
import std.db.kv as kv
import std.bytes as buffers
from std.bytes import BytesError

def main() -> Result[(), kv.KvError | BytesError | IoError]:
    mut store = kv.memory()?
    key = buffers.from_text("user:42")?
    value = buffers.from_text("Ada")?
    mut transaction = store.begin()?
    store.put("names", &key, &value)?
    transaction.commit()?
    drop(transaction)

    match store.get("names", &key)?:
        case Some(name):
            print(name.to_text()?)?
        case Nothing:
            print("not found")?

    mut rows = store.scan_all("names", max_rows=20)?
    while True:
        match rows.next()?:
            case Some(entry):
                print(entry.key.to_text()?, entry.value.to_text()?)?
            case Nothing:
                break
    drop(rows)
    store.close()?
    Ok(())
```
```output
Ada
user:42 Ada
```

This example uses a private in-memory store. For persistence, replace
`kv.memory()` with `kv.connect("records.db")`; the default creates a new store or
opens an existing KV file. It rejects a database belonging to another schema.
`Mode.ReadOnly` opens an existing store without allowing writes.

Keys and values need not be text; the example converts them because names
are UTF-8. Every returned value owns its buffers. Scans require an explicit row
limit and copy one row at a time. Finish/drop a scan before modifying the
same Store or ending its transaction. Use `scan(table, start, stop, max_rows)`
for a byte range with inclusive start and exclusive stop.

The [reference](../../design/stdlib/kv.md) explains file compatibility,
per-row size limits, lock conflicts and scan lifetimes. The
[engine comparison](../../design/stdlib/kv-engine.md) explains why this API
uses SQLite rather than introducing a redb dependency.
