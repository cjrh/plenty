//! `std.db.sqlite`: typed bindings over the vendored SQLite library.

const HEADER: &str = r#"import std.bytes
import std.db.sqlite
from std.bytes import Bytes
from std.db.sqlite import Begin, Connection, DbError, Mode, Statement, ValueType

# "ok", or the error: a SQLite failure as its extended code, primary code
# and message, anything else by name.
def report[T](result: Result[T, DbError]) -> Result[(), Failure]:
    match result:
        case Ok(_):
            print("ok")?
        case Err(error):
            match &error:
                case DbError.Sqlite(cause):
                    print("Sqlite", cause.code, cause.primary(), cause.message)?
                case _:
                    print(str.repr(error)?)?
    Ok(())

def count(db: &mut Connection, sql: &str) -> Result[i64, DbError]:
    mut query = db.prepare(sql)?
    query.step()?
    query.integer(0)
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}def main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

fn build(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    (dir, executable)
}

fn run(source: &str) -> std::process::Output {
    let (_dir, executable) = build(source);
    std::process::Command::new(executable).output().unwrap()
}

fn output(source: &str) -> String {
    let output = run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("std.db.sqlite.DbError.", "")
        .replace("std.db.sqlite.ValueType.", "")
        .replace("std.db.sqlite.DbError", "E")
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

/// A fresh directory for database files, and a Plenty string literal naming
/// `name` inside it.
fn database(name: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    let literal = format!("{:?}", path.to_str().unwrap());
    (dir, literal)
}

#[test]
fn values_round_trip_with_their_storage_class() {
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
db.execute("CREATE TABLE t(a, b, c, d, e)")?
mut insert = db.prepare("INSERT INTO t VALUES (?, ?, ?, ?, ?)")?
data = std.bytes.from_list(&[0u8, 255u8, 0u8]?)?
insert.bind_integer(1, -9223372036854775807 - 1)?
insert.bind_real(2, 0.1)?
insert.bind_text(3, "naïve ☃")?
insert.bind_blob(4, &data)?
insert.bind_null(5)?
insert.run()?
insert.bind_integer(1, 9223372036854775807)?
insert.bind_real(2, -2.5)?
insert.bind_text(3, "")?
insert.bind_blob(4, &std.bytes.empty()?)?
insert.bind_integer(5, 7)?
insert.run()?
print(db.changes(), db.last_insert_rowid())?
mut query = db.prepare("SELECT a, b, c, d, e AS last FROM t ORDER BY rowid")?
print(query.column_count(), query.column_name(0)?, query.column_name(4)?)?
while query.step()?:
    blob = query.blob(3)?
    print(query.integer(0)?, query.real(1)?, str.repr(query.text(2)?)?, blob.len(), str.repr(query.optional_integer(4)?)?)?
    print(str.repr(query.kind(3)?)?, query.is_null(4)?)?
print(query.step()?)?
"#,
        r#"
1 2
5 a last
-9223372036854775808 0.1 "naïve ☃" 3 Option[i64].Nothing
Blob True
9223372036854775807 -2.5 "" 0 Option[i64].Some(7)
Blob False
False
"#,
    );
}

#[test]
fn typed_reads_do_not_convert() {
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
mut query = db.prepare("SELECT 1, 'one', 1.5, NULL")?
report(query.integer(0))?
query.step()?
report(query.text(0))?
report(query.integer(1))?
report(query.real(0))?
report(query.integer(3))?
report(query.optional_text(3))?
report(query.blob(1))?
report(query.integer(4))?
report(query.integer(-1))?
report(query.column_name(9))?
query.step()?
report(query.integer(0))?
"#,
        r#"
NoRow
TypeMismatch(Integer)
TypeMismatch(Text)
TypeMismatch(Integer)
TypeMismatch(Null)
ok
TypeMismatch(Text)
OutOfRange
OutOfRange
OutOfRange
NoRow
"#,
    );
}

#[test]
fn bound_values_never_become_sql() {
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
db.execute("CREATE TABLE users(name TEXT NOT NULL)")?
hostile = "x'); DROP TABLE users; --"
mut insert = db.prepare("INSERT INTO users(name) VALUES (:name)")?
index = insert.parameter_index(":name")?
print(insert.parameter_count(), index)?
insert.bind_text(index, hostile)?
insert.run()?
mut find = db.prepare("SELECT count(*) FROM users WHERE name = ?1")?
find.bind_text(1, hostile)?
find.step()?
print(find.integer(0)?)?
print(count(&mut db, "SELECT count(*) FROM sqlite_master")?)?
report(insert.parameter_index(":missing"))?
"#,
        "1 1\n1\n1\nOutOfRange",
    );
}

#[test]
fn statement_errors_are_values() {
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
db.execute("CREATE TABLE t(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)")?
report(db.execute("CREATE TABLE t(x)"))?
report(db.prepare("SELEC 1"))?
report(db.prepare("  -- only a comment"))?
report(db.prepare("SELECT 1; SELECT 2"))?
report(db.prepare("SELECT 1;  -- trailing comment"))?
mut insert = db.prepare("INSERT INTO t(name) VALUES (?)")?
report(insert.bind_text(2, "x"))?
report(insert.bind_real(1, 0.0 / 0.0))?
report(insert.run())?
insert.bind_text(1, "a")?
insert.run()?
report(insert.run())?
print(count(&mut db, "SELECT count(*) FROM t")?)?
"#,
        r#"
Sqlite 1 1 table t already exists
Sqlite 1 1 near "SELEC": syntax error
EmptySql
TrailingSql
ok
OutOfRange
OutOfRange
Sqlite 1299 19 NOT NULL constraint failed: t.name
Sqlite 2067 19 UNIQUE constraint failed: t.name
1
"#,
    );
}

#[test]
fn a_finished_statement_stays_finished_until_reset() {
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
report(db.execute("SELECT 1"))?
mut rows = db.prepare("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 2) SELECT x FROM n")?
print(rows.step()?, rows.step()?, rows.step()?, rows.step()?)?
rows.reset()
print(rows.step()?, rows.integer(0)?)?
mut add = db.prepare("SELECT ?1 + 1")?
add.bind_integer(1, 1)?
add.step()?
print(add.integer(0)?)?
add.bind_integer(1, 41)?
add.step()?
print(add.integer(0)?)?
add.clear_bindings()?
add.step()?
print(add.is_null(0)?)?
"#,
        "ok\nTrue True False False\nTrue 1\n2\n42\nTrue",
    );
}

#[test]
fn transactions_commit_roll_back_and_roll_back_on_drop() {
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
db.execute("CREATE TABLE t(x)")?
mut tx = db.begin(Begin.Deferred)?
report(db.begin(Begin.Immediate))?
db.execute("INSERT INTO t VALUES (1)")?
tx.commit()?
report(tx.commit())?
report(tx.rollback())?
mut undone = db.begin(Begin.Immediate)?
db.execute("INSERT INTO t VALUES (2)")?
undone.rollback()?
dropped = db.begin(Begin.Exclusive)?
db.execute("INSERT INTO t VALUES (3)")?
drop(dropped)
print(count(&mut db, "SELECT count(*) FROM t")?)?
mut outside = db.begin(Begin.Deferred)?
db.execute("COMMIT")?
report(outside.commit())?
mut again = db.begin(Begin.Deferred)?
again.commit()?
"#,
        "TransactionActive\nTransactionFinished\nTransactionFinished\n1\nTransactionFinished",
    );
}

#[test]
fn files_modes_and_paths() {
    let (_dir, path) = database("app.db");
    let (_missing_dir, missing) = database("missing.db");
    native(
        &format!(
            r#"
mut db = std.db.sqlite.connect({path}, Mode.Create, 4096)?
db.execute("CREATE TABLE t(x); INSERT INTO t VALUES (1)")?
db.close()?
report(db.execute("SELECT 1"))?
mut reader = std.db.sqlite.connect({path}, Mode.ReadOnly, 4096)?
print(count(&mut reader, "SELECT x FROM t")?)?
report(reader.execute("INSERT INTO t VALUES (2)"))?
report(std.db.sqlite.connect({missing}, Mode.ReadWrite, 4096))?
report(std.db.sqlite.connect("", Mode.Create, 4096))?
report(std.db.sqlite.connect(":memory:", Mode.Create, 4096))?
report(std.db.sqlite.connect("a\0b", Mode.Create, 4096))?
report(std.db.sqlite.memory(1023))?
report(std.db.sqlite.memory(1000000001))?
"#
        ),
        r#"
Closed
1
Sqlite 8 8 attempt to write a readonly database
Sqlite 14 14 unable to open database file
InvalidPath
InvalidPath
InvalidPath
OutOfRange
OutOfRange
"#,
    );
}

#[test]
fn busy_contention_fails_fast_and_waits_when_asked() {
    let (_dir, path) = database("shared.db");
    native(
        &format!(
            r#"
mut first = std.db.sqlite.connect({path}, Mode.Create, 4096)?
first.execute("CREATE TABLE t(x)")?
mut second = std.db.sqlite.connect({path}, Mode.ReadWrite, 4096)?
mut writing = first.begin(Begin.Immediate)?
first.execute("INSERT INTO t VALUES (1)")?
report(second.begin(Begin.Immediate))?
second.set_busy_timeout(20)?
report(second.execute("INSERT INTO t VALUES (2)"))?
report(second.set_busy_timeout(-1))?
writing.commit()?
mut later = second.begin(Begin.Immediate)?
second.execute("INSERT INTO t VALUES (2)")?
later.commit()?
print(count(&mut first, "SELECT count(*) FROM t")?)?
"#
        ),
        "Sqlite 5 5 database is locked\nSqlite 5 5 database is locked\nOutOfRange\n2",
    );
}

#[test]
fn closing_waits_for_statements_and_transactions() {
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
query = db.prepare("SELECT 1")?
report(db.close())?
drop(query)
tx = db.begin(Begin.Deferred)?
report(db.close())?
drop(tx)
db.close()?
db.close()?
report(db.prepare("SELECT 1"))?
report(db.begin(Begin.Deferred))?
report(db.set_busy_timeout(1))?
print(db.changes())?
"#,
        "InUse\nInUse\nClosed\nClosed\nClosed\n0",
    );
}

#[test]
fn statements_keep_their_connection_alive() {
    let source = format!(
        r#"{HEADER}
def prepared() -> Result[Statement, DbError]:
    mut db = std.db.sqlite.memory(4096)?
    db.prepare("SELECT 7")

def main() -> Result[(), Failure]:
    mut query = prepared()?
    query.step()?
    print(query.integer(0)?)?
    Ok(())
"#
    );
    assert_eq!(output(&source), "7");
}

#[test]
fn values_beyond_the_limit_are_rejected() {
    let (_dir, path) = database("big.db");
    native(
        &format!(
            r#"
mut wide = std.db.sqlite.connect({path}, Mode.Create, 100000)?
wide.execute("CREATE TABLE t(x)")?
wide.execute("INSERT INTO t VALUES (zeroblob(2000))")?
mut narrow = std.db.sqlite.connect({path}, Mode.ReadWrite, 1024)?
mut small = narrow.prepare("INSERT INTO t VALUES (?)")?
mut data = std.bytes.with_capacity(1025)?
data.resize(1025, 0u8)?
report(small.bind_blob(1, &data))?
data.resize(1000, 0u8)?
small.bind_blob(1, &data)?
small.run()?
report(narrow.execute("INSERT INTO t VALUES (zeroblob(1025))"))?
mut read = narrow.prepare("SELECT length(x) FROM t ORDER BY rowid")?
report(read.step())?
report(std.db.sqlite.memory(1023))?
"#
        ),
        "TooLarge\nTooLarge\nTooLarge\nOutOfRange",
    );
}

#[test]
fn invalid_stored_text_is_reported() {
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
mut query = db.prepare("SELECT CAST(x'ff' AS TEXT)")?
query.step()?
report(query.text(0))?
"#,
        "InvalidUtf8",
    );
}

#[test]
fn handles_cannot_be_forged() {
    let output = run(&program(
        "fake = __bytes_new(0)?\n\
         print(__sqlite_step(fake))?\n",
    ));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("operand is not a std.db.sqlite statement"),
        "{stderr}"
    );
}

#[test]
fn connections_work_in_worker_threads() {
    let (_dir, path) = database("worker.db");
    let source = format!(
        r#"{HEADER}
def work(path: str) -> Result[(), Failure]:
    mut db = std.db.sqlite.connect(&path, Mode.ReadWrite, 4096)?
    mut tx = db.begin(Begin.Immediate)?
    db.execute("INSERT INTO t VALUES ('from a thread')")?
    tx.commit()?
    report(db.execute("SELECT * FROM missing"))?
    Ok(())

def main() -> Result[(), Failure]:
    mut db = std.db.sqlite.connect({path}, Mode.Create, 4096)?
    db.execute("CREATE TABLE t(x TEXT)")?
    with spawn(work, {path})?:
        pass
    mut query = db.prepare("SELECT x FROM t")?
    query.step()?
    print(query.text(0)?)?
    Ok(())
"#
    );
    assert_eq!(
        output(&source),
        "Sqlite 1 1 no such table: missing\nfrom a thread"
    );
}

/// SQLite sits behind its own runtime entry and is compiled with function
/// and data sections, so `--gc-sections` drops it from programs that never
/// open a database.
#[test]
fn programs_without_databases_do_not_link_sqlite() {
    use object::{Object, ObjectSymbol, SymbolKind};
    let symbols = |source: &str| {
        let (_dir, executable) = build(source);
        let data = std::fs::read(&executable).unwrap();
        let file = object::File::parse(&*data).unwrap();
        let count = file
            .symbols()
            .filter(|symbol| symbol.kind() != SymbolKind::File)
            .filter_map(|symbol| symbol.name().ok().map(str::to_owned))
            .filter(|name| name == "plenty_sqlite" || name.starts_with("sqlite3"))
            .count();
        (count, data.len())
    };
    let plain = "def main() -> Result[(), Failure]:\n    print(\"hi\")?\n    Ok(())\n";
    let (count, plain_size) = symbols(plain);
    assert_eq!(count, 0);
    let (count, size) = symbols(&program(
        "std.db.sqlite.memory(4096)?.execute(\"SELECT 1\")?\n",
    ));
    assert!(count > 0);
    assert!(size > plain_size + 500_000, "{size} vs {plain_size}");
}

#[test]
fn the_vendored_sources_are_the_documented_release() {
    let root = env!("CARGO_MANIFEST_DIR");
    let header = std::fs::read_to_string(format!(
        "{root}/plenty-runtime/native/sqlite-3.54.0/sqlite3.h"
    ))
    .unwrap();
    assert!(header.contains("#define SQLITE_VERSION        \"3.54.0\""));
    let reference =
        std::fs::read_to_string(format!("{root}/book/src/design/stdlib/sqlite.md")).unwrap();
    assert!(reference.contains("3.54.0"));
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
mut query = db.prepare("SELECT sqlite_version()")?
query.step()?
print(query.text(0)?)?
"#,
        "3.54.0",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_is_reported_without_leaks() {
    // Each of the runtime's own allocations (the path copy, the connection,
    // statement and transaction owners, a copied-out text) fails in turn.
    // The exit-time balance check verifies nothing leaks.
    for after in 0..6 {
        let body = format!(
            r#"
print("__test_fail_allocations_after_{after}__")?
result = attempt()
print("__test_restore_allocations__")?
report(result)?
"#
        );
        let source = program(&body).replace(
            "def main()",
            r#"def attempt() -> Result[str, DbError]:
    mut db = std.db.sqlite.memory(4096)?
    mut query = db.prepare("SELECT 'a longer text value'")?
    mut tx = db.begin(Begin.Deferred)?
    query.step()?
    text = query.text(0)?
    tx.commit()?
    Ok(text)

def main()"#,
        );
        let out = output(&source);
        assert!(
            out == "ok" || out == "Allocation(AllocError.OutOfMemory)",
            "after {after}: {out}"
        );
        if after < 4 {
            assert_eq!(out, "Allocation(AllocError.OutOfMemory)", "after {after}");
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn stepping_and_reading_numbers_does_not_allocate_in_the_runtime() {
    native(
        r#"
mut db = std.db.sqlite.memory(4096)?
db.execute("CREATE TABLE t(x INTEGER, y REAL)")?
mut insert = db.prepare("INSERT INTO t VALUES (?1, ?2)")?
mut query = db.prepare("SELECT x, y FROM t")?
mut total = 0
print("__test_begin_no_allocations__")?
for i in range(0, 100):
    insert.bind_integer(1, i)?
    insert.bind_real(2, 0.5)?
    insert.run()?
while query.step()?:
    total += query.integer(0)?
print("__test_end_no_allocations__")?
print(total)?
"#,
        "4950",
    );
}
