# Store records in SQLite

`std.db.sqlite` keeps data in a SQLite database: a single file, or memory.
Values travel only through prepared statements, so a value is never
mistaken for SQL.

## Insert and query

`execute` runs fixed SQL such as a schema. `prepare` compiles one statement
with `?` parameters; bind each by number, from 1, and `run` it. A query is
read a row at a time with `step`, and each column, numbered from 0, is read
with the method for its type:

```plenty
import std.db.sqlite

def main() -> Result[(), AllocError | IoError | std.db.sqlite.DbError]:
    mut db = std.db.sqlite.memory(65536)?
    db.execute("CREATE TABLE books(title TEXT NOT NULL, year INTEGER, rating REAL)")?
    mut insert = db.prepare("INSERT INTO books VALUES (?, ?, ?)")?
    insert.bind_text(1, "Dune")?
    insert.bind_integer(2, 1965)?
    insert.bind_real(3, 4.3)?
    insert.run()?
    insert.bind_text(1, "x'); DROP TABLE books; --")?
    insert.bind_null(2)?
    insert.bind_real(3, 1.0)?
    insert.run()?
    mut query = db.prepare("SELECT title, year FROM books WHERE rating > ? ORDER BY rating")?
    query.bind_real(1, 0.5)?
    while query.step()?:
        print(query.text(0)?, query.optional_integer(1)?)?
    Ok(())
```
```output
x'); DROP TABLE books; -- Option[i64].Nothing
Dune Option[i64].Some(1965)
```

The second title looks like an attack but is stored as plain text. Reads
copy each value out, so a `str` from `text` stays valid after the next
`step`. `memory(65536)` sets the largest text, blob or row the connection
will store or read; anything larger is `TooLarge`.

## Types are checked

Each read requires the column's stored type. A NULL needs `optional_integer`
or another optional read, and a number stored as text is not an integer:

```plenty
import std.db.sqlite

def main() -> Result[(), AllocError | IoError | std.db.sqlite.DbError]:
    mut db = std.db.sqlite.memory(65536)?
    mut query = db.prepare("SELECT '42', NULL")?
    query.step()?
    print(str.repr(query.integer(0))?)?
    print(str.repr(query.integer(1))?)?
    print(str.repr(query.kind(0)?)?)?
    Ok(())
```
```output
Result[i64, std.db.sqlite.DbError].Err(std.db.sqlite.DbError.TypeMismatch(std.db.sqlite.ValueType.Text))
Result[i64, std.db.sqlite.DbError].Err(std.db.sqlite.DbError.TypeMismatch(std.db.sqlite.ValueType.Null))
std.db.sqlite.ValueType.Text
```

## Transactions

`begin` returns a `Transaction`. `commit` keeps its changes; dropping it
without `commit`, including by returning early with `?`, rolls them back.
SQLite failures carry SQLite's own code and message:

```plenty
import std.db.sqlite
from std.db.sqlite import Begin, Connection, DbError

def transfer(db: &mut Connection, amount: i64) -> Result[(), DbError]:
    mut tx = db.begin(Begin.Immediate)?
    mut change = db.prepare("UPDATE accounts SET balance = balance + ? WHERE name = ?")?
    change.bind_integer(1, 0 - amount)?
    change.bind_text(2, "alice")?
    change.run()?
    change.bind_integer(1, amount)?
    change.bind_text(2, "bob")?
    change.run()?
    tx.commit()

def main() -> Result[(), AllocError | IoError | DbError]:
    mut db = std.db.sqlite.memory(65536)?
    db.execute("CREATE TABLE accounts(name TEXT PRIMARY KEY, balance INTEGER CHECK (balance >= 0))")?
    db.execute("INSERT INTO accounts VALUES ('alice', 10), ('bob', 0)")?
    transfer(&mut db, 4)?
    match transfer(&mut db, 50):
        case Ok(_):
            print("moved")?
        case Err(DbError.Sqlite(error)):
            print(error.primary() == std.db.sqlite.constraint, error.message)?
        case Err(_):
            print("other")?
    mut query = db.prepare("SELECT name, balance FROM accounts ORDER BY name")?
    while query.step()?:
        print(query.text(0)?, query.integer(1)?)?
    Ok(())
```
```output
True CHECK constraint failed: balance >= 0
alice 6
bob 4
```

The failed transfer left no trace: its transaction rolled back when
`transfer` returned the error.

For a file, use `std.db.sqlite.connect(path, Mode.Create, limit)`. Another
connection holding a write lock makes a write fail at once with SQLite's
busy code; `set_busy_timeout` makes it wait instead. See the
[`std.db.sqlite` reference](../../design/stdlib/sqlite.md) for every error,
the busy policy, and how connections are shared between threads.

SQLite's FTS5 full-text search is built in. Applications that need a native
extension, such as vector search, can load a trusted shared library with
`Connection.load_extension(path, entrypoint="")`. This runs native code in the
application; SQL cannot enable extension loading. See
[native extensions and full-text search](../../design/stdlib/sqlite.md#native-extensions-and-full-text-search)
for the loading contract and connection lifetime.
