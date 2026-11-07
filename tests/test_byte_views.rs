//! Issue #843: a loop that cannot resize or replace a `Bytes` buffer reads its
//! length and first-byte address once, so `get` and `set` in the loop need no
//! length call, owner reload, signed test or native call. An index the loop
//! cannot prove in range still traps through the method itself.
use std::path::Path;
use std::process::Command;

mod support;

const KERNEL: &str = r#"
import std.bytes

def transform(data: &mut std.bytes.Bytes) -> ():
    for i in range(data.len()):
        data.set(i, data.get(i) ^ 32u8)

def main() -> Result[(), AllocError | IoError]:
    mut data = std.bytes.from_text("ACGT")?
    transform(&mut data)
    print(data)?
    Ok(())
"#;

fn plenty(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// The `--dump` file `kind` of `source`, compiled with `flags`.
fn dump(source: &str, flags: &[&str], kind: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let mut args = flags.to_vec();
    args.extend(["--dump", kind, "main.plenty", "-o", "out"]);
    plenty(dir.path(), &args);
    std::fs::read_to_string(dir.path().join(format!("out/{kind}.txt"))).unwrap()
}

#[test]
fn range_loop_reads_bytes_directly() {
    assert_eq!(support::run_file_stdout(KERNEL), "b\"acgt\"");
    let asm = dump(KERNEL, &["--release"], "asm");
    let body = support::asm_body(&asm, "transform");
    let calls = |symbol: &str| {
        body.lines()
            .filter(|line| support::native_call(line) && line.contains(symbol))
            .count()
    };
    // One length and one address read before the loop; no access calls.
    assert_eq!(calls("plenty_bytes_len"), 1, "{body}");
    assert_eq!(calls("plenty_byte_view_rows"), 1, "{body}");
    for symbol in ["plenty_bytes_get", "plenty_bytes_set", "plenty_trap"] {
        assert_eq!(calls(symbol), 0, "{symbol}:\n{body}");
    }
    // The hoisted `len()` names its call site in its own trap records
    // (issue #842), so the function sets no caller context.
    assert_eq!(calls("plenty_caller_enter"), 0, "{body}");
}

const RESIZING: &str = r#"
import std.bytes

class Holder:
    data: std.bytes.Bytes

def grow(data: &mut std.bytes.Bytes) -> Result[(), AllocError]:
    data.push(1u8)?
    Ok(())

def pushes(data: &mut std.bytes.Bytes) -> Result[u8, AllocError]:
    mut total = 0u8
    for i in range(4):
        total = total ^ data.get(i)
        data.push(7u8)?
        data.set(i, 9u8)
    Ok(total)

def calls(data: &mut std.bytes.Bytes) -> Result[u8, AllocError]:
    mut total = 0u8
    for i in range(4):
        grow(data)?
        total = total ^ data.get(i)
    Ok(total)

def field(holder: &mut Holder) -> ():
    for i in range(holder.data.len()):
        holder.data.set(i, holder.data.get(i) ^ 1u8)

def owned() -> Result[u8, AllocError]:
    mut data = std.bytes.from_text("abcd")?
    mut total = 0u8
    for i in range(data.len()):
        data.set(i, data.get(i) ^ 32u8)
    for i in range(data.len()):
        total = total ^ data.get(i)
    Ok(total)

def shared(data: &std.bytes.Bytes) -> u8:
    mut total = 0u8
    for i in range(data.len()):
        total = total ^ data.get(i)
    total

def nested(data: &mut std.bytes.Bytes) -> Result[i64, AllocError]:
    mut total = 0
    for j in range(2):
        for i in range(data.len()):
            data.set(i, data.get(i) ^ 2u8)
        data.push(48u8)?
        total = total ^ data.len()
    Ok(total)

def main() -> Result[(), AllocError | IoError]:
    mut data = std.bytes.from_text("ABCD")?
    print(pushes(&mut data)?)?
    print(data)?
    print(calls(&mut data)?)?
    print(data.len())?
    mut holder = Holder(data=std.bytes.from_text("abc")?)
    field(&mut holder)
    print(holder.data)?
    print(owned()?)?
    print(shared(&holder.data))?
    mut more = std.bytes.from_text("xy")?
    print(nested(&mut more)?)?
    print(more)?
    Ok(())
"#;

// A loop that may resize the buffer, directly or through a call, keeps the
// method calls; the other receivers read the buffer once per loop.
#[test]
fn only_loops_that_cannot_resize_cache_bytes() {
    let expected =
        "4\nb\"\\t\\t\\t\\t\\x07\\x07\\x07\\x07\"\n0\n12\nb\"`cb\"\n4\n97\n7\nb\"xy20\"\n";
    for flags in [&["--fast-compile"][..], &["--release"]] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.plenty"), RESIZING).unwrap();
        let mut args = flags.to_vec();
        args.push("main.plenty");
        assert_eq!(plenty(dir.path(), &args), expected, "{flags:?}");
    }
    let ops = dump(RESIZING, &["--release"], "ops");
    let cached = |name: &str| support::asm_body(&ops, name).contains("BytesGetView");
    for name in ["field", "owned", "shared", "nested"] {
        assert!(cached(name), "{name}:\n{}", support::asm_body(&ops, name));
    }
    for name in ["pushes", "calls"] {
        assert!(!cached(name), "{name}:\n{}", support::asm_body(&ops, name));
    }
    // Issue #844: the inlined methods read the one-word owner through
    // `&mut Bytes`; a two-word load would keep a dead high-word load.
    let clif = dump(RESIZING, &["--release"], "clif-opt");
    for name in ["pushes", "calls"] {
        let body = support::asm_body(&clif, name);
        assert!(!body.contains("load.i128"), "{name}:\n{body}");
    }
}

const REPLACING: &str = r#"
import std.bytes
from std.bytes import Bytes

class Holder:
    data: Bytes

    def grow(self: &mut Holder) -> Result[(), AllocError]:
        self.data.push(1u8)?
        Ok(())

    def method(self: &mut Holder) -> Result[u8, AllocError]:
        mut total = 0u8
        for i in range(self.data.len()):
            total = total ^ self.data.get(i)
            if i == 0:
                self.grow()?
        Ok(total)

def assigned(holder: &mut Holder) -> Result[u8, AllocError]:
    mut total = 0u8
    for i in range(holder.data.len()):
        total = total ^ holder.data.get(i)
        holder.data = std.bytes.from_text("zzzzz")?
    Ok(total)

def replaced(holder: &mut Holder) -> Result[u8, AllocError]:
    mut total = 0u8
    for i in range(holder.data.len()):
        total = total ^ holder.data.get(i)
        old = replace(holder.data, std.bytes.from_text("qqqqq")?)
        drop(old)
    Ok(total)

def truncated(data: &mut Bytes) -> u8:
    mut total = 0u8
    for i in range(data.len()):
        total = total ^ data.get(i)
        data.truncate(data.len())
    total

def inner_clears(data: &mut Bytes) -> u8:
    mut total = 0u8
    for j in range(data.len()):
        total = total ^ data.get(j)
        for k in range(1):
            if total == 0u8:
                data.clear()
    total

def main() -> Result[(), AllocError | IoError]:
    mut holder = Holder(data=std.bytes.from_text("abcd")?)
    print(holder.method()?)?
    print(holder.data.len())?
    print(assigned(&mut holder)?)?
    print(replaced(&mut holder)?)?
    mut data = std.bytes.from_text("abc")?
    print(truncated(&mut data))?
    print(inner_clears(&mut data))?
    print(data.len())?
    Ok(())
"#;

// Every way the loop can resize or replace the buffer keeps the method
// calls: an owner method, assignment, `replace`, `truncate`, and an inner
// loop that clears the buffer the outer loop reads.
#[test]
fn loops_that_resize_or_replace_keep_method_calls() {
    let expected = "4\n5\n97\n122\n96\n96\n3\n";
    for flags in [&["--fast-compile"][..], &["--release"]] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.plenty"), REPLACING).unwrap();
        let mut args = flags.to_vec();
        args.push("main.plenty");
        assert_eq!(plenty(dir.path(), &args), expected, "{flags:?}");
    }
    let ops = dump(REPLACING, &["--release"], "ops");
    for name in [
        "Holder.method",
        "assigned",
        "replaced",
        "truncated",
        "inner_clears",
    ] {
        let body = support::asm_body(&ops, name);
        assert!(!body.is_empty(), "{name} missing from ops dump");
        assert!(!body.contains("BytesGetView"), "{name}:\n{body}");
    }
}

const TRAPS: &str = r#"
import std.bytes

def read(data: &std.bytes.Bytes, start: i64, stop: i64) -> u8:
    mut total = 0u8
    for i in range(start, stop):
        total = total ^ data.get(i)
    total

def write(data: &mut std.bytes.Bytes, start: i64, stop: i64) -> ():
    for i in range(start, stop):
        data.set(i, 1u8)

def main() -> Result[(), AllocError | IoError]:
    mut data = std.bytes.from_text("ABCD")?
    match args()?[1]:
        case "0":
            print(read(&data, -1, 4))?
        case "1":
            print(read(&data, 0, 5))?
        case "2":
            print(read(&data, 9223372036854775805, 9223372036854775807))?
        case "3":
            write(&mut data, -2, 4)
        case "4":
            write(&mut data, 0, 5)
        case "5":
            write(&mut data, 9223372036854775806, 9223372036854775807)
        case _:
            print(read(&data, 0, 4))?
    Ok(())
"#;

// The cached access traps through the method: its message, std position and
// the caller's position are those of an uncached call.
#[test]
fn unproven_cached_indexes_trap_at_the_call() {
    let get = "error: std/bytes.plenty:48:13: Bytes.get: index is outside 0..len()\n\
               note: called from main.plenty:7:25\n";
    let set = "error: std/bytes.plenty:53:13: Bytes.set: index is outside 0..len()\n\
               note: called from main.plenty:12:9\n";
    let ops = dump(TRAPS, &["--release"], "ops");
    assert!(support::asm_body(&ops, "read").contains("BytesGetView"));
    assert!(support::asm_body(&ops, "write").contains("BytesSetView"));
    for flags in [&["--fast-compile"][..], &["--release"]] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.plenty"), TRAPS).unwrap();
        let mut args = flags.to_vec();
        args.extend(["--compile", "main.plenty", "-o", "program"]);
        plenty(dir.path(), &args);
        let program = dir.path().join("program");
        for (case, expected) in [get, get, get, set, set, set].iter().enumerate() {
            let output = Command::new(&program)
                .arg(case.to_string())
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{flags:?} case {case}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                *expected,
                "{flags:?} case {case}"
            );
        }
        let output = Command::new(&program).arg("6").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), "4\n");
    }
}
