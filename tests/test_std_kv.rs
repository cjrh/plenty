//! Ordered byte storage over SQLite, including transactions and format guards.
mod support;

const HEADER: &str = r#"import std.db.kv as kv
import std.db.sqlite as sqlite
import std.bytes as buffers
import std.tempfile
import std.path
from std.bytes import BytesError
from std.fs import FsError

def aborted(store: &mut kv.Store) -> Result[(), kv.KvError | BytesError]:
    mut transaction = store.begin()?
    key = buffers.from_text("aborted")?
    value = buffers.from_text("discard")?
    store.put("data", &key, &value)?
    Err(kv.KvError.InvalidName)

def dump(scan: &mut kv.Scan) -> Result[(), kv.KvError | BytesError | IoError]:
    while True:
        match scan.next()?:
            case Some(entry):
                print(entry.key.to_list()?, entry.value.to_list()?)?
            case Nothing:
                break
    Ok(())

"#;

fn native(body: &str, expected: &str) {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    let source = format!(
        "{HEADER}def main() -> Result[(), kv.KvError | sqlite.DbError | AllocError | IoError | BytesError | FsError]:\n{body}    Ok(())\n"
    );
    let actual = support::run_file_stdout(&source);
    let actual = actual
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(actual, expected.trim_matches('\n'), "{source}");
}

#[test]
fn names_are_bound_data_and_blobs_are_ordered_lexicographically() {
    native(
        r#"
mut store = kv.memory()?
table = "quotes'\"; DROP TABLE plenty_kv; --\0"
value = buffers.from_bytes(b"\xff\0")?
for literal in [b"b", b"\xff", b"", b"a\0", b"a"]?:
    key = buffers.from_bytes(literal)?
    store.put(&table, &key, &value)?
key = buffers.from_bytes(b"a")?
replacement = buffers.from_bytes(b"new")?
store.put(&table, &key, &replacement)?
store.put("other", &key, &value)?
print(store.len(&table)?, store.len("missing")?, store.contains("other", &key)?)?
mut scan = store.scan_all(&table, 10)?
dump(&mut scan)?
drop(scan)
match store.get(&table, &key)?:
    case Some(found):
        print(found.to_text()?)?
    case Nothing:
        print("missing")?
print(store.remove(&table, &key)?, store.remove(&table, &key)?, store.clear(&table)?, store.len("other")?)?
store.close()?
"#,
        "5 0 True\n[] [255, 0]\n[97] [110, 101, 119]\n[97, 0] [255, 0]\n[98] [255, 0]\n[255] [255, 0]\nnew\nTrue False 4 1",
    );
}

#[test]
fn scans_have_bounds_limits_direction_and_owned_rows() {
    native(
        r#"
mut store = kv.memory()?
empty = buffers.empty()?
for literal in [b"a", b"b", b"c", b"d"]?:
    key = buffers.from_bytes(literal)?
    store.put("t", &key, &empty)?
start = buffers.from_bytes(b"b")?
stop = buffers.from_bytes(b"d")?
mut scan = store.scan("t", &start, &stop, 10)?
dump(&mut scan)?
mut scan_after = store.scan("t", &start, &stop, 10, after=True)?
dump(&mut scan_after)?
mut backwards = store.scan_all("t", 2, reverse=True)?
dump(&mut backwards)?
mut zero = store.scan_all("t", 0)?
print(zero.next()?.is_none(), zero.next()?.is_none())?
mut single = store.scan_all("t", 1)?
entry = single.next()?.unwrap()
drop(single)
print(entry.key.to_text()?, entry.value.len())?
"#,
        "[98] []\n[99] []\n[99] []\n[100] []\n[99] []\nTrue True\na 0",
    );
}

#[test]
fn transactions_commit_rollback_and_propagate_without_partial_changes() {
    native(
        r#"
mut store = kv.memory()?
key = buffers.from_text("committed")?
value = buffers.from_text("value")?
mut transaction = store.begin()?
store.put("data", &key, &value)?
transaction.commit()?
drop(transaction)
mut discarded = store.begin()?
drop(store.clear("data")?)
discarded.rollback()?
drop(discarded)
match aborted(&mut store):
    case Err(_):
        print("rolled back")?
    case Ok(_):
        print("unexpected success")?
print(store.len("data")?, store.contains("data", &key)?)?
mut outer = store.begin()?
match store.begin():
    case Err(_):
        print("nested rejected")?
    case Ok(_):
        print("unexpected success")?
outer.rollback()?
"#,
        "rolled back\n1 True\nnested rejected",
    );
}

#[test]
fn persistent_files_reopen_and_interoperate_with_sqlite() {
    native(
        r#"
directory = std.tempfile.directory()?
path = directory.path().join(&std.path.from_text("store.db")?)?.to_text()?
key = buffers.from_text("hello")?
value = buffers.from_text("world")?
mut store = kv.connect(&path)?
store.put("t", &key, &value)?
store.close()?
mut reader = kv.connect(&path, kv.Mode.ReadOnly)?
print(reader.contains("t", &key)?)?
match reader.put("t", &key, &value):
    case Err(error):
        match error:
            case kv.KvError.Database(cause):
                match cause:
                    case sqlite.DbError.Sqlite(problem):
                        print(problem.primary() == sqlite.read_only)?
                    case _:
                        print(False)?
            case _:
                print(False)?
    case Ok(_):
        print(False)?
reader.close()?
mut sql = sqlite.connect(&path, sqlite.Mode.ReadOnly, 4096)?
mut statement = sql.prepare("SELECT hex(key),hex(value) FROM plenty_kv WHERE table_name='t'")?
drop(statement.step()?)
print(statement.text(0)?, statement.text(1)?)?
"#,
        "True\nTrue\n68656C6C6F 776F726C64",
    );
}

#[test]
fn format_guards_reject_foreign_files_and_newer_versions() {
    native(
        r#"
directory = std.tempfile.directory()?
path = directory.path().join(&std.path.from_text("store.db")?)?.to_text()?
mut sql = sqlite.connect(&path, sqlite.Mode.Create, 4096)?
sql.execute("CREATE TABLE sqlitexuser(value INTEGER); INSERT INTO sqlitexuser VALUES(7)")?
sql.close()?
match kv.connect(&path):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
mut altered = sqlite.connect(&path, sqlite.Mode.ReadWrite, 4096)?
mut count = altered.prepare("SELECT value FROM sqlitexuser")?
drop(count.step()?)
print(count.integer(0)?)?
drop(count)
altered.execute("DROP TABLE sqlitexuser; PRAGMA application_id=1347177302; PRAGMA user_version=99")?
altered.close()?
match kv.connect(&path):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
"#,
        "std.db.kv.KvError.ForeignDatabase\n7\nstd.db.kv.KvError.UnsupportedVersion(99)",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_preserves_scan_position_and_put_is_atomic() {
    native(
        r#"
mut store = kv.memory()?
key = buffers.from_text("a")?
value = buffers.from_text("original")?
replacement = buffers.from_text("replacement")?
store.put("t", &key, &value)?
mut scan = store.scan_all("t", 1)?
print("__test_fail_allocations_after_0__")?
failed = scan.next()
print("__test_restore_allocations__")?
match failed:
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
entry = scan.next()?.unwrap()
print(entry.key.to_text()?, entry.value.to_text()?)?
drop(scan)
print("__test_fail_allocations_after_0__")?
failed_put = store.put("t", &key, &replacement)
print("__test_restore_allocations__")?
match failed_put:
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
print(store.get("t", &key)?.unwrap().to_text()?)?
"#,
        "std.db.kv.KvError.Allocation(AllocError.OutOfMemory)\na original\nstd.db.kv.KvError.Allocation(AllocError.OutOfMemory)\noriginal",
    );
}

#[test]
fn concurrent_connections_lock_and_observe_only_committed_changes() {
    native(
        r#"
directory = std.tempfile.directory()?
path = directory.path().join(&std.path.from_text("store.db")?)?.to_text()?
mut first = kv.connect(&path)?
mut second = kv.connect(&path, kv.Mode.ReadWrite)?
key = buffers.from_text("key")?
value = buffers.from_text("value")?
mut transaction = first.begin()?
first.put("t", &key, &value)?
print(second.contains("t", &key)?)?
match second.begin():
    case Err(error):
        match error:
            case kv.KvError.Database(cause):
                match cause:
                    case sqlite.DbError.Sqlite(problem):
                        print(problem.primary() == sqlite.busy)?
                    case _:
                        print(False)?
            case _:
                print(False)?
    case Ok(_):
        print(False)?
transaction.commit()?
drop(transaction)
print(second.contains("t", &key)?)?
mut scan = second.scan_all("t", 10)?
match second.close():
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
drop(scan)
second.close()?
first.close()?
"#,
        "False\nTrue\nTrue\nstd.db.kv.KvError.Database(std.db.sqlite.DbError.InUse)",
    );
}

#[test]
fn limits_and_invalid_names_fail_without_writes() {
    native(
        r#"
mut store = kv.memory(1024)?
key = buffers.empty()?
mut value = buffers.empty()?
value.resize(1500, 0u8)?
match store.put("t", &key, &value):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
match store.put("", &key, &key):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
match store.scan_all("t", -1):
    case Err(error):
        print(error)?
    case Ok(_):
        print("unexpected success")?
print(store.len("t")?)?
"#,
        "std.db.kv.KvError.Database(std.db.sqlite.DbError.TooLarge)\nstd.db.kv.KvError.InvalidName\nstd.db.kv.KvError.InvalidLimit\n0",
    );
}
