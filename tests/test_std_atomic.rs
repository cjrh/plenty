//! `std.atomic`: inline atomics, compile-time orderings, and scoped sharing.
mod support;
use support::check_file_error as rejected;

const HEADER: &str = "from std.atomic import Atomic, Ordering, fence\n";

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{HEADER}def main() -> Result[(), Failure]:\n{body}    Ok(())\n")
}

fn build(source: &str) -> (tempfile::TempDir, std::process::Output) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = std::process::Command::new("timeout")
        .arg("60s")
        .arg(&executable)
        .output()
        .unwrap();
    (dir, output)
}

fn output(source: &str) -> String {
    let (_dir, output) = build(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string()
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        output(&program(body)),
        expected.trim_matches('\n'),
        "{body}"
    );
}

fn rejects(body: &str, message: &str) {
    let error = rejected(&program(body));
    assert!(error.contains(message), "{body}\n{error}");
}

#[test]
fn integer_methods_return_previous_values() {
    native(
        r#"
a = Atomic[u32](10)
print(a.load(Ordering.Relaxed))?
a.store(20, Ordering.Release)
print(a.swap(30, Ordering.AcqRel))?
print(a.fetch_add(5, Ordering.SeqCst))?
print(a.fetch_sub(15, Ordering.Relaxed))?
print(a.fetch_and(12, Ordering.Relaxed))?
print(a.fetch_or(3, Ordering.Relaxed))?
print(a.fetch_xor(1, Ordering.Relaxed))?
print(a.fetch_nand(6, Ordering.Relaxed))?
print(a.load(Ordering.Acquire))?
print(a.into_inner())?
"#,
        "10\n20\n30\n35\n20\n4\n7\n6\n4294967289\n4294967289",
    );
}

#[test]
fn every_width_uses_its_own_range() {
    native(
        r#"
a = Atomic[i8](-128)
print(a.fetch_wrapping_sub(1, Ordering.Relaxed))?
print(a.load(Ordering.Relaxed))?
b = Atomic[u8](255)
print(b.fetch_wrapping_add(2, Ordering.Relaxed))?
print(b.load(Ordering.Relaxed))?
c = Atomic[i16](-2)
print(c.fetch_add(1, Ordering.Relaxed))?
print(c.into_inner())?
d = Atomic[u16](65535)
print(d.swap(0, Ordering.Relaxed))?
e = Atomic[i32](-7)
print(e.fetch_sub(-7, Ordering.Relaxed))?
print(e.load(Ordering.Relaxed))?
f = Atomic[i64](i64.MIN)
print(f.fetch_wrapping_sub(1, Ordering.Relaxed))?
print(f.load(Ordering.Relaxed))?
g = Atomic[u64](u64.MAX)
print(g.fetch_wrapping_add(1, Ordering.Relaxed))?
print(g.load(Ordering.Relaxed))?
"#,
        "-128\n127\n255\n1\n-2\n-1\n65535\n-7\n0\n-9223372036854775808\n9223372036854775807\n18446744073709551615\n0",
    );
}

#[test]
fn max_and_min_follow_signedness() {
    native(
        r#"
s = Atomic[i8](-1)
print(s.fetch_max(1, Ordering.Relaxed))?
print(s.load(Ordering.Relaxed))?
print(s.fetch_min(-100, Ordering.Relaxed))?
print(s.load(Ordering.Relaxed))?
u = Atomic[u8](255)
print(u.fetch_max(1, Ordering.Relaxed))?
print(u.load(Ordering.Relaxed))?
print(u.fetch_min(3, Ordering.Relaxed))?
print(u.load(Ordering.Relaxed))?
"#,
        "-1\n1\n1\n-100\n255\n255\n255\n3",
    );
}

#[test]
fn compare_exchange_reports_both_outcomes() {
    native(
        r#"
a = Atomic[i64](5)
match a.compare_exchange(5, 8, Ordering.AcqRel, Ordering.Acquire):
    case Ok(previous):
        print(f"replaced {previous}"?)?
    case Err(actual):
        print(f"kept {actual}"?)?
match a.compare_exchange(5, 9, Ordering.SeqCst, Ordering.Relaxed):
    case Ok(previous):
        print(f"replaced {previous}"?)?
    case Err(actual):
        print(f"kept {actual}"?)?
print(a.load(Ordering.SeqCst))?
b = Atomic[bool](False)
print(str.repr(b.compare_exchange(False, True, Ordering.Relaxed, Ordering.Relaxed))?)?
print(str.repr(b.compare_exchange(False, True, Ordering.Relaxed, Ordering.Relaxed))?)?
c = Atomic[i8](-5)
print(str.repr(c.compare_exchange(-5, -6, Ordering.SeqCst, Ordering.Relaxed))?)?
print(str.repr(c.compare_exchange(-5, -7, Ordering.SeqCst, Ordering.Relaxed))?)?
"#,
        "replaced 5\nkept 8\n8\nResult[bool, bool].Ok(False)\nResult[bool, bool].Err(True)\nResult[i8, i8].Ok(-5)\nResult[i8, i8].Err(-6)",
    );
}

#[test]
fn bool_methods_keep_values_boolean() {
    native(
        r#"
a = Atomic[bool](True)
print(a.fetch_not(Ordering.Relaxed))?
print(a.load(Ordering.Relaxed))?
print(a.fetch_or(True, Ordering.Relaxed))?
print(a.fetch_and(False, Ordering.Relaxed))?
print(a.fetch_xor(True, Ordering.Relaxed))?
print(a.fetch_nand(True, Ordering.Relaxed))?
print(a.load(Ordering.Relaxed))?
print(a.fetch_nand(False, Ordering.Relaxed))?
print(a.load(Ordering.Relaxed))?
print(a.fetch_nand(True, Ordering.Relaxed))?
print(a.load(Ordering.Relaxed) == False)?
print(a.swap(True, Ordering.Relaxed))?
a.store(False, Ordering.SeqCst)
print(a.into_inner())?
"#,
        "True\nFalse\nFalse\nTrue\nFalse\nTrue\nFalse\nFalse\nTrue\nTrue\nTrue\nFalse\nFalse",
    );
}

#[test]
fn fetch_add_and_fetch_sub_trap_on_overflow() {
    for (body, line) in [
        ("a = Atomic[u8](250)\nprint(a.fetch_add(5, Ordering.Relaxed))?\nprint(a.fetch_add(1, Ordering.Relaxed))?", 5),
        ("a = Atomic[i16](-32767)\nprint(a.fetch_sub(1, Ordering.Relaxed))?\nprint(a.fetch_sub(1, Ordering.Relaxed))?", 5),
        ("a = Atomic[u64](0)\na.fetch_sub(1, Ordering.SeqCst)", 4),
    ] {
        let source = program(body);
        let (_dir, output) = build(&source);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{source}\n{stderr}");
        assert!(
            stderr.contains(&format!("main.plenty:{line}:")) && stderr.contains("integer overflow"),
            "{source}\n{stderr}"
        );
    }
}

#[test]
fn scoped_workers_count_exactly() {
    let source = format!(
        r#"{HEADER}
def work(hits: &Atomic[u64], rounds: i64) -> ():
    for _ in range(rounds):
        hits.fetch_add(1, Ordering.Relaxed)
        hits.fetch_wrapping_sub(1, Ordering.Relaxed)
        hits.fetch_wrapping_add(1, Ordering.Relaxed)

def main() -> Result[(), Failure]:
    hits = Atomic[u64](0)
    with spawn(work, &hits, 250000)? as a, spawn(work, &hits, 250000)? as b, spawn(work, &hits, 250000)? as c, spawn(work, &hits, 250000)? as d:
        hits.load(Ordering.Acquire)
        a.join()
        b.join()
        c.join()
        d.join()
    print(hits.load(Ordering.Acquire))?
    Ok(())
"#
    );
    assert_eq!(output(&source), "1000000");
}

#[test]
fn checked_updates_retry_under_contention() {
    let source = format!(
        r#"{HEADER}
def work(total: &Atomic[i64], rounds: i64) -> ():
    for _ in range(rounds):
        total.fetch_add(3, Ordering.Relaxed)
        total.fetch_sub(1, Ordering.Relaxed)

def main() -> Result[(), Failure]:
    total = Atomic[i64](0)
    with spawn(work, &total, 200000)?, spawn(work, &total, 200000)?, spawn(work, &total, 200000)?, spawn(work, &total, 200000)?:
        pass
    print(total.load(Ordering.Acquire))?
    Ok(())
"#
    );
    assert_eq!(output(&source), "1600000");
}

#[test]
fn workers_update_atomics_in_every_storage_shape() {
    let source = format!(
        r#"{HEADER}
def flip(flags: &list[Atomic[bool]], index: i64) -> ():
    flags[index].store(True, Ordering.Release)

def lower(values: &list[Atomic[i8]]) -> ():
    for _ in range(10):
        values[0].fetch_sub(3, Ordering.Relaxed)
        values[1].fetch_min(-100, Ordering.Relaxed)

def bump(triple: &(Atomic[u8], Atomic[u8], u8)) -> ():
    triple[0].fetch_add(triple[2], Ordering.Relaxed)
    triple[1].fetch_wrapping_sub(1, Ordering.Relaxed)
    pass

def mark(slot: &Option[Atomic[u8]]) -> ():
    match slot:
        case Some(value):
            value.fetch_or(8, Ordering.Relaxed)
            pass
        case Nothing:
            pass

def main() -> Result[(), Failure]:
    flags = [Atomic(False), Atomic(False), Atomic(False)]?
    with spawn(flip, &flags, 1)?:
        pass
    for flag in flags:
        print(flag.load(Ordering.Acquire))?
    values = [Atomic[i8](-1), Atomic[i8](-7)]?
    with spawn(lower, &values)?, spawn(lower, &values)?:
        pass
    print(values[0].load(Ordering.Relaxed))?
    print(values[1].load(Ordering.Relaxed))?
    triple = (Atomic[u8](1), Atomic[u8](0), 5u8)
    with spawn(bump, &triple)?, spawn(bump, &triple)?:
        pass
    print(triple[0].load(Ordering.Relaxed))?
    print(triple[1].load(Ordering.Relaxed))?
    print(triple[2])?
    slot: Option[Atomic[u8]] = Some(Atomic(3))
    with spawn(mark, &slot)?:
        pass
    match slot:
        case Some(value):
            print(value.load(Ordering.Relaxed))?
        case Nothing:
            pass
    mut items = [Atomic[u32](1), Atomic[u32](2)]?
    items.append(Atomic[u32](7))?
    print(items.pop().unwrap().into_inner())?
    sender, receiver = channel[Atomic[u64]](1)?
    sender.send(Atomic[u64](41))?
    received = receiver.recv()?
    print(received.fetch_add(1, Ordering.Relaxed) + received.load(Ordering.Relaxed))?
    Ok(())
"#
    );
    assert_eq!(
        output(&source),
        "False\nTrue\nFalse\n-61\n-100\n11\n254\n5\n11\n7\n83"
    );
}

#[test]
fn borrowed_atomics_cannot_move() {
    let worker = "def work(a: &Atomic[u64]) -> ():\n    a.fetch_add(1, Ordering.Relaxed)\n    pass\n\nclass Holder:\n    hits: Atomic[u64]\n\n";
    for (body, message) in [
        (
            "a = Atomic[u64](0)\n    with spawn(work, &a)?:\n        b = a",
            "conflicting borrow: cannot move `a` while it is borrowed",
        ),
        (
            "mut a = Atomic[u64](0)\n    with spawn(work, &a)?:\n        a = Atomic[u64](5)",
            "conflicting borrow: cannot assign to `a` while it is borrowed",
        ),
        (
            "h = Holder(Atomic(0))\n    with spawn(work, &h.hits)?:\n        g = h",
            "conflicting borrow: cannot move `h` while `h.hits` is borrowed",
        ),
    ] {
        let source =
            format!("{HEADER}{worker}def main() -> Result[(), Failure]:\n    {body}\n    Ok(())\n");
        let error = rejected(&source);
        assert!(error.contains(message), "{source}\n{error}");
    }
}

#[test]
fn class_fields_are_shared_by_reference() {
    let source = format!(
        r#"{HEADER}
class Stats:
    hits: Atomic[u32]
    largest: Atomic[i64]
    seen: Atomic[bool]

    def record(self, value: i64) -> ():
        self.hits.fetch_add(1, Ordering.Relaxed)
        self.largest.fetch_max(value, Ordering.Relaxed)
        self.seen.store(True, Ordering.Release)

def work(stats: &Stats, base: i64) -> ():
    for i in range(50000):
        stats.record(base + i)

def main() -> Result[(), Failure]:
    stats = Stats(hits=Atomic[u32](0), largest=Atomic(i64.MIN), seen=Atomic(False))
    with spawn(work, &stats, 0)?, spawn(work, &stats, 1000000)?, spawn(work, &stats, -5)?:
        pass
    print(stats.hits.load(Ordering.Acquire))?
    print(stats.largest.load(Ordering.Acquire))?
    print(stats.seen.load(Ordering.Acquire))?
    Ok(())
"#
    );
    assert_eq!(output(&source), "150000\n1049999\nTrue");
}

#[test]
fn atomics_live_in_ordinary_owners() {
    native(
        r#"
pair = (Atomic[u16](1), 5)
print(pair[0].fetch_add(2, Ordering.Relaxed))?
print(pair[0].load(Ordering.Relaxed))?
mut items = [Atomic[i32](0), Atomic[i32](1)]?
items[1].fetch_add(5, Ordering.Relaxed)
items.append(Atomic[i32](9))?
print(items[1].load(Ordering.Relaxed) + items[2].load(Ordering.Relaxed))?
boxed = Box(Atomic[i8](-1))?
print((*boxed).fetch_min(-8, Ordering.Relaxed))?
maybe: Option[Atomic[u8]] = Some(Atomic(7))
match maybe:
    case Some(value):
        print(value.fetch_or(8, Ordering.Relaxed))?
        print(value.into_inner())?
    case Nothing:
        pass
moved = Atomic[u64](3)
again = moved
print(again.load(Ordering.Relaxed))?
mut replaced = Atomic[u64](1)
replaced = Atomic[u64](4)
print(replaced.load(Ordering.Relaxed))?
print(Atomic[i64](6).into_inner())?
fence(Ordering.Release)
fence(Ordering.Acquire)
fence(Ordering.AcqRel)
fence(Ordering.SeqCst)
"#,
        "1\n3\n15\n-1\n7\n15\n3\n4\n6",
    );
}

#[test]
fn constructor_takes_its_type_from_context() {
    native(
        r#"
a: Atomic[u8] = Atomic(200)
print(a.fetch_wrapping_add(100, Ordering.Relaxed))?
print(a.load(Ordering.Relaxed))?
b = Atomic(-3)
print(b.fetch_sub(1, Ordering.Relaxed))?
print(b.load(Ordering.Relaxed) == -4)?
"#,
        "200\n44\n-3\nTrue",
    );
}

#[test]
fn import_aliases_name_the_same_facility() {
    let source = r#"import std.atomic as atomic
from std.atomic import Atomic as Cell, Ordering as Order, fence as barrier

def bump(value: &atomic.Atomic[i32]) -> i32:
    value.fetch_add(2, atomic.Ordering.SeqCst)

def main() -> Result[(), Failure]:
    x = atomic.Atomic[i32](-3)
    print(bump(&x))?
    y: Cell[i32] = Cell(1)
    print(bump(&y))?
    print(x.load(Order.Relaxed) + y.load(atomic.Ordering.Acquire))?
    atomic.fence(Order.SeqCst)
    barrier(atomic.Ordering.Release)
    Ok(())
"#;
    assert_eq!(output(source), "-3\n1\n2");
}

#[test]
fn names_resolve_only_through_an_import() {
    for (source, message) in [
        (
            "def main() -> ():\n    a = Atomic[u64](0)\n",
            "`Atomic` is not a generic function",
        ),
        (
            "def f(a: &Atomic[u64]) -> ():\n    pass\ndef main() -> ():\n    pass\n",
            "unknown type `Atomic`",
        ),
        (
            "def main() -> ():\n    fence(Ordering.SeqCst)\n",
            "unknown function `fence`",
        ),
        (
            "from std.atomic import Atomic\ndef main() -> ():\n    a = Atomic[u64](0)\n    a.load(Ordering.Relaxed)\n",
            "load requires its ordering written as `Ordering.<variant>` from std.atomic",
        ),
    ] {
        let error = rejected(source);
        assert!(error.contains(message), "{source}\n{error}");
    }
}

#[test]
fn invalid_orderings_are_compile_errors() {
    for (call, message) in [
        ("a.load(Ordering.Release)", "load does not accept Ordering.Release; use Ordering.Relaxed, Ordering.Acquire, or Ordering.SeqCst"),
        ("a.load(Ordering.AcqRel)", "load does not accept Ordering.AcqRel"),
        ("a.store(1, Ordering.Acquire)", "store does not accept Ordering.Acquire; use Ordering.Relaxed, Ordering.Release, or Ordering.SeqCst"),
        ("a.store(1, Ordering.AcqRel)", "store does not accept Ordering.AcqRel"),
        ("a.compare_exchange(0, 1, Ordering.SeqCst, Ordering.Release)", "compare_exchange failure ordering does not accept Ordering.Release"),
        ("a.compare_exchange(0, 1, Ordering.Relaxed, Ordering.AcqRel)", "compare_exchange failure ordering does not accept Ordering.AcqRel"),
        ("fence(Ordering.Relaxed)", "fence does not accept Ordering.Relaxed; use Ordering.Release, Ordering.Acquire, Ordering.AcqRel, or Ordering.SeqCst"),
        ("a.load(Ordering.Strict)", "Ordering has no variant `Strict`"),
        ("order = Ordering.Relaxed\na.load(order)", "load requires its ordering written as `Ordering.<variant>`"),
        ("a.fetch_add(1, (Ordering.Relaxed if True else Ordering.SeqCst))", "fetch_add requires its ordering written"),
        ("a.load()", "load takes an Ordering"),
        ("a.compare_exchange(0, 1, Ordering.SeqCst)", "compare_exchange takes current and new values and success and failure Orderings"),
    ] {
        rejects(&format!("a = Atomic[u64](0)\n{call}"), message);
    }
}

#[test]
fn atomics_have_no_value_operations() {
    for (body, message) in [
        (
            "a = Atomic[u64](0)\nb = Atomic[u64](0)\nsame = a == b",
            "values containing atomics do not support equality",
        ),
        (
            "a = Atomic[u64](0)\nb = a < a",
            "operator `<` does not accept Atomic[u64]",
        ),
        ("a = Atomic[u64](0)\nprint(a)?", "atomics cannot be printed"),
        (
            "a = Atomic[u64](0)\nprint(f\"{a}\"?)?",
            "atomics cannot be formatted",
        ),
        (
            "a = Atomic[u64](0)\ntext = str.repr(a)?",
            "atomics cannot be formatted",
        ),
        (
            "a = Atomic[u64](0)\nb = copy(a)",
            "this resource cannot be copied",
        ),
        (
            "a = (Atomic[u64](0), 1)\nb = copy(a)",
            "this resource cannot be copied",
        ),
        (
            "a = [Atomic[u8](0)]?\nfound = Atomic[u8](0) in a",
            "values containing atomics do not support membership equality",
        ),
        (
            "a = Atomic[u64](0)\nb = a\na.load(Ordering.Relaxed)",
            "use of moved binding `a`",
        ),
        (
            "x = Atomic[u64](0).load(Ordering.Relaxed)",
            "atomic operations need a place to update; bind the atomic to a name first",
        ),
        (
            "a = Atomic[bool](False)\na.fetch_add(True, Ordering.Relaxed)",
            "Atomic[bool] has no method `fetch_add`",
        ),
        (
            "a = Atomic[bool](False)\na.fetch_max(True, Ordering.Relaxed)",
            "Atomic[bool] has no method `fetch_max`",
        ),
        (
            "a = Atomic[u64](0)\na.fetch_not(Ordering.Relaxed)",
            "Atomic[u64] has no method `fetch_not`",
        ),
        (
            "a = Atomic[u64](0)\na.store(True, Ordering.Relaxed)",
            "expected u64",
        ),
    ] {
        rejects(body, message);
    }
}

#[test]
fn references_reach_atomics_only_through_methods() {
    for (function, message) in [
        (
            "def f(r: &Atomic[u64]) -> u64:\n    *r\n",
            "an atomic is read with load(), not through `*`",
        ),
        (
            "def f(r: &mut Atomic[u64]) -> ():\n    *r = Atomic[u64](3)\n",
            "an atomic is written with store(), not through `*`",
        ),
        (
            "def f(r: &Atomic[u64]) -> u64:\n    r.into_inner()\n",
            "into_inner consumes an owned atomic, not a reference",
        ),
    ] {
        let source = format!("{HEADER}{function}def main() -> ():\n    pass\n");
        let error = rejected(&source);
        assert!(error.contains(message), "{source}\n{error}");
    }
}

#[test]
fn unsupported_element_types_name_the_allowed_ones() {
    let allowed = "the element type must be bool, i8, i16, i32, i64, u8, u16, u32, or u64";
    for body in [
        "a = Atomic[f64](0.0)",
        "a = Atomic(\"text\")",
        "a = Atomic[Atomic[u8]](Atomic[u8](0))",
    ] {
        rejects(body, allowed);
    }
    let source =
        format!("{HEADER}def f(a: &Atomic[str]) -> ():\n    pass\ndef main() -> ():\n    pass\n");
    let error = rejected(&source);
    assert!(
        error.contains(&format!("Atomic[str] is not supported; {allowed}")),
        "{error}"
    );
    let generic = format!(
        "{HEADER}def made[T](value: T) -> T:\n    a = Atomic[T](value)\n    a.swap(value, Ordering.Relaxed)\n\ndef main() -> Result[(), Failure]:\n    print(made(-9i16))?\n    print(made(True))?\n    Ok(())\n"
    );
    assert_eq!(output(&generic), "-9\nTrue");
    let error = rejected(&generic.replace("made(True)", "made(\"text\")"));
    assert!(
        error.contains("cannot instantiate `made[str]`")
            && error.contains(&format!("Atomic[str] is not supported; {allowed}")),
        "{error}"
    );
}
