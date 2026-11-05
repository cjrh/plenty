//! Explicit list element references (issues #378 and #739): `&xs[i]` and
//! `&mut xs[i]` compute the row address inline instead of calling the
//! generic collection dispatcher. References keep their semantics: loans,
//! writes through `&mut`, returning an element, nested and non-scalar rows,
//! and the same out-of-bounds diagnostic and site.
mod support;

use std::path::Path;
use std::process::{Command, Output};

const PROGRAM: &str = r#"class Point:
    x: i64
    y: i64

def bump(xs: &mut list[i64], i: i64) -> Result[(), OverflowError[i64]]:
    r = &mut xs[i]
    *r = *r + 1
    Ok(())

def pick(xs: &list[str], i: i64) -> &str:
    &xs[i]

def rename(xs: &mut list[str], i: i64, name: str) -> ():
    r = &mut xs[i]
    *r = name

def shift(points: &mut list[Point], i: i64) -> Result[(), OverflowError[i64]]:
    p = &mut points[i]
    p.x = p.x + p.y
    Ok(())

def toggle(flags: &mut list[bool], i: i64) -> ():
    f = &mut flags[i]
    *f = not *f

def wrap(bytes: &mut list[u8], i: i64) -> ():
    b = &mut bytes[i]
    *b = b.wrapping_add(10u8)

def halve(values: &mut list[f32], i: i64) -> ():
    v = &mut values[i]
    *v = *v / 2.0f32

def scale(grid: &mut list[list[i64]], i: i64, j: i64) -> Result[(), OverflowError[i64]]:
    c = &mut grid[i][j]
    *c = *c * 100
    Ok(())

def element(xs: &list[i64], i: i64) -> &i64:
    &xs[i]

def read(value: &i64) -> i64:
    *value

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut xs = [1, 2, 3]?
    bump(&mut xs, 0)?
    bump(&mut xs, -1)?
    print(xs, read(element(&xs, 1)), *element(&xs, -3))?
    r = &xs[2]
    print(*r)?
    xs.append(5)?
    print(xs)?

    mut names = ["a", "b"]?
    print(pick(&names, 0), pick(&names, -1))?
    rename(&mut names, 1, "cde")
    print(names, len(*pick(&names, 1)))?

    mut points = [Point(1, 2), Point(3, 4)]?
    shift(&mut points, -1)?
    print(points[0].x, points[1].x)?

    mut flags = [True, False]?
    toggle(&mut flags, 1)
    print(flags)?
    mut bytes = [1u8, 250u8]?
    wrap(&mut bytes, 1)
    print(bytes)?
    mut halves = [1.0f32, 3.0f32]?
    halve(&mut halves, 1)
    print(halves)?

    # Growing an inner list does not move the outer list's rows.
    mut grid = [[1, 2]?, [3]?]?
    scale(&mut grid, 0, 1)?
    scale(&mut grid, -1, -1)?
    inner = &mut grid[1]
    for i in range(100):
        inner.append(i)?
    print(grid[0], len(grid[1]), grid[1][0])?
    Ok(())
"#;

const EXPECTED: &str = r#"[2, 2, 4] 2 2
4
[2, 2, 4, 5]
a b
["a", "cde"] 3
1 7
[True, True]
[1, 4]
[1.0, 1.5]
[1, 200] 101 300
"#;

fn plenty(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plenty"))
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

fn stdout(dir: &Path, args: &[&str]) -> String {
    let output = plenty(dir, args);
    assert!(
        output.status.success(),
        "plenty {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn setup(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("refs.plenty"), source).unwrap();
    dir
}

#[test]
fn element_references_keep_reference_semantics() {
    let dir = setup(PROGRAM);
    assert_eq!(stdout(dir.path(), &["refs.plenty"]), EXPECTED);
    assert_eq!(stdout(dir.path(), &["--release", "refs.plenty"]), EXPECTED);
}

#[test]
fn scalar_backed_class_references_preserve_adjacent_rows() {
    let cases = [
        ("i8", "-11i8", "-22i8", "-33i8"),
        ("i16", "-111i16", "-222i16", "-333i16"),
        ("i32", "-111i32", "-222i32", "-333i32"),
        ("i64", "-4294967297", "-8589934594", "-12884901891"),
        ("u8", "11u8", "222u8", "133u8"),
        ("u16", "111u16", "222u16", "333u16"),
        ("u32", "111u32", "222u32", "333u32"),
        ("u64", "4294967297u64", "8589934594u64", "12884901891u64"),
        ("f32", "1.5f32", "2.5f32", "3.5f32"),
        ("f64", "1.5", "2.5", "3.5"),
        ("bool", "False", "True", "False"),
    ];
    let mut source = String::new();
    for (ty, a, b, c) in cases {
        source.push_str(&format!(r#"
@copy
class Atom_{ty}:
    value: {ty}
@copy
class Nested_{ty}:
    value: Atom_{ty}
class Owned_{ty}:
    value: {ty}

def check_{ty}() -> Result[(), AllocError | IoError]:
    mut values = [Atom_{ty}({a}), Atom_{ty}({b}), Atom_{ty}({c})]?
    values[0] = values[-1]
    print(values[0].value == {c}, values[1].value == {b}, values[2].value == {c})?
    reference = &mut values[1]
    reference.value = {a}
    print(values[0].value == {c}, values[1].value == {a}, values[2].value == {c})?
    old = replace(values[-1], Atom_{ty}({b}))
    print(old.value == {c}, values[1].value == {a}, values[2].value == {b})?
    mut nested = [Nested_{ty}(Atom_{ty}({a})), Nested_{ty}(Atom_{ty}({b})), Nested_{ty}(Atom_{ty}({c}))]?
    nested[0] = nested[-1]
    print(nested[0].value.value == {c}, nested[1].value.value == {b}, nested[2].value.value == {c})?
    mut owned = [Owned_{ty}({a}), Owned_{ty}({b}), Owned_{ty}({c})]?
    owned[0] = Owned_{ty}({c})
    print(owned[0].value == {c}, owned[1].value == {b}, owned[2].value == {c})?
    mut maybe = Some(Nested_{ty}(Atom_{ty}({a})))
    match &mut maybe:
        case Some(holder):
            previous = replace(holder.value, Atom_{ty}({b}))
            print(previous.value == {a}, holder.value.value == {b}, True)?
        case Nothing:
            print(False, False, False)?
    print(maybe.is_some(), maybe.unwrap().value.value == {b}, True)?
    Ok(())
"#));
    }
    source.push_str("\ndef main() -> Result[(), AllocError | IoError]:\n");
    for (ty, _, _, _) in cases {
        source.push_str(&format!("    check_{ty}()?\n"));
    }
    source.push_str("    Ok(())\n");
    let dir = setup(&source);
    let expected = "True True True\n".repeat(cases.len() * 7);
    for args in [
        &["--fast-compile", "refs.plenty"][..],
        &["--release", "refs.plenty"],
    ] {
        assert_eq!(stdout(dir.path(), args), expected);
    }
}

#[test]
fn parallel_owned_scalar_wrappers_do_not_clear_neighboring_rows() {
    let source = r#"
class Byte:
    value: u8
class Number:
    value: f64
class Nested:
    value: Byte

def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError]:
    with ThreadPoolExecutor(2, 1)? as pool:
        print(iter([Byte(11u8), Byte(22u8), Byte(33u8), Byte(44u8)]?).par(&pool, chunks=2).map(def(item: Byte) -> u8: item.value).collect()?)?
        print(iter([Number(1.5), Number(2.5), Number(3.5), Number(4.5)]?).par(&pool, chunks=2).map(def(item: Number) -> f64: item.value).collect()?)?
        print(iter([Nested(Byte(11u8)), Nested(Byte(22u8)), Nested(Byte(33u8)), Nested(Byte(44u8))]?).par(&pool, chunks=2).map(def(item: Nested) -> u8: item.value.value).collect()?)?
    Ok(())
"#;
    let dir = setup(source);
    for args in [
        &["--fast-compile", "refs.plenty"][..],
        &["--release", "refs.plenty"],
    ] {
        assert_eq!(
            stdout(dir.path(), args),
            "[11, 22, 33, 44]\n[1.5, 2.5, 3.5, 4.5]\n[11, 22, 33, 44]\n"
        );
    }
}

/// The generic dispatcher reported these messages and sites; the narrow
/// helper must report the same ones.
#[test]
fn out_of_bounds_references_keep_the_diagnostic_and_site() {
    let cases = [
        (
            "def pick(xs: &list[str], i: i64) -> &str:\n    &xs[i]\n\ndef main() -> Result[(), AllocError | IoError]:\n    names = [\"a\", \"b\"]?\n    print(pick(&names, -2))?\n    print(pick(&names, 2))?\n    Ok(())\n",
            "a\n",
            "error: refs.plenty:2:6: index out of bounds: index 2, length 2\n",
        ),
        (
            "def main() -> Result[(), AllocError | IoError]:\n    mut grid = [[1, 2]?, [3]?]?\n    r = &mut grid[0][1]\n    *r = 9\n    print(grid)?\n    q = &mut grid[1][-2]\n    *q = 1\n    Ok(())\n",
            "[[1, 9], [3]]\n",
            "error: refs.plenty:6:14: index out of bounds: index -2, length 1\n",
        ),
        (
            "def main() -> Result[(), AllocError | IoError]:\n    empty: list[i64] = []?\n    r = &empty[0]\n    print(*r)?\n    Ok(())\n",
            "",
            "error: refs.plenty:3:10: index out of bounds: index 0, length 0\n",
        ),
    ];
    for (source, expected_stdout, expected_stderr) in cases {
        let dir = setup(source);
        for args in [
            &["--fast-compile", "refs.plenty"][..],
            &["--release", "refs.plenty"],
        ] {
            let output = plenty(dir.path(), args);
            assert!(!output.status.success(), "{source}");
            assert_eq!(String::from_utf8_lossy(&output.stdout), expected_stdout);
            assert_eq!(String::from_utf8_lossy(&output.stderr), expected_stderr);
        }
    }
}

#[test]
fn element_references_hold_the_list_loan() {
    let shared = support::check_error(
        "mut xs = [1, 2].unwrap()\nr = &xs[0]\nxs.append(3).unwrap()\nprint(*r).unwrap()\n",
    );
    assert!(shared.contains("while `xs[0]` is borrowed"), "{shared}");
    let exclusive = support::check_error(
        "mut xs = [1, 2].unwrap()\nr = &mut xs[0]\nprint(len(xs)).unwrap()\n*r = 3\n",
    );
    assert!(
        exclusive.contains("while `xs[0]` is exclusively borrowed"),
        "{exclusive}"
    );
}

/// Explicit element references compute the row address inline and keep
/// their bounds check.
#[test]
fn element_references_compute_rows_inline() {
    let dir = setup(PROGRAM);
    for function in ["bump", "pick", "rename", "shift", "scale", "element"] {
        let asm = stdout(
            dir.path(),
            &[
                "--release",
                "--dump",
                "asm",
                "--dump-filter",
                function,
                "refs.plenty",
                "-o",
                "-",
            ],
        );
        assert!(!asm.contains("# plenty_list_row\n"), "{function}: {asm}");
        assert!(asm.contains("plenty_trap_list_index"), "{function}: {asm}");
        assert!(!asm.contains("plenty_op_"), "{function}: {asm}");
    }
}

/// Field access on `list[@copy class]` rows in loops (issue #739): exclusive,
/// shared and owned lists use the loop's cached header, and a loop that can
/// resize the list reads the header at each access.
const ROWS: &str = r#"@copy
class Body:
    x: f64
    v: f64
    n: i64


def step(bodies: &mut list[Body], dt: f64) -> Result[(), OverflowError[i64]]:
    for i in range(len(bodies)):
        bodies[i].x += dt * bodies[i].v
        for j in range(i + 1, len(bodies)):
            bodies[j].v -= bodies[i].x
            bodies[i].n += 1
    Ok(())


def total(bodies: &list[Body]) -> f64:
    mut sum = 0.0
    for i in range(len(bodies)):
        sum += bodies[i].x + bodies[i].v
    sum


def grow(bodies: &mut list[Body]) -> Result[(), AllocError | AppendError[Body] | OverflowError[i64]]:
    for i in range(2):
        bodies[i].n += 10
        bodies.append(Body(1.0, 1.0, i))?
    Ok(())


def main() -> Result[(), AllocError | AppendError[Body] | IoError | OverflowError[i64]]:
    mut bodies = [Body(1.0, 2.0, 0), Body(3.0, 4.0, 0), Body(5.0, 6.0, 0)]?
    step(&mut bodies, 0.5)?
    print(f"{total(&bodies)} {bodies[0].n} {bodies[1].n} {bodies[2].n}"?)?
    mut owned = [Body(1.0, 1.0, 0), Body(2.0, 2.0, 0)]?
    for i in range(len(owned)):
        owned[i].x *= 2.0
        owned[i].n = i
    print(f"{owned[0].x} {owned[1].x} {owned[1].n}"?)?
    grow(&mut bodies)?
    print(f"{len(bodies)} {bodies[0].n} {bodies[1].n} {bodies[4].n}"?)?
    Ok(())
"#;

fn ops(dir: &Path, function: &str) -> String {
    stdout(
        dir,
        &[
            "--release",
            "--dump",
            "ops",
            "--dump-filter",
            function,
            "refs.plenty",
            "-o",
            "-",
        ],
    )
}

#[test]
fn record_rows_in_loops_keep_their_results() {
    let dir = setup(ROWS);
    for args in [
        &["--fast-compile", "refs.plenty"][..],
        &["--release", "refs.plenty"],
    ] {
        assert_eq!(
            stdout(dir.path(), args),
            "15.0 2 1 0\n2.0 4.0 1\n5 12 11 1\n"
        );
    }
}

#[test]
fn record_rows_use_the_loop_cache_unless_the_loop_resizes() {
    let dir = setup(ROWS);
    let total = ops(dir.path(), "total");
    assert_eq!(total.matches("ElementRefView(").count(), 2, "{total}");
    assert!(!total.contains("ElementRef("), "{total}");
    // The inner loop's accesses are cached; the outer loop's are not, since
    // only innermost loops cache.
    let step = ops(dir.path(), "step");
    assert_eq!(step.matches("ElementRefView(").count(), 5, "{step}");
    assert_eq!(step.matches("ElementRef(").count(), 3, "{step}");
    let grow = ops(dir.path(), "grow");
    assert!(grow.contains("ElementRef("), "{grow}");
    assert!(!grow.contains("ElementRefView("), "{grow}");
}

/// The cached loop calls nothing, and the list header is read before it.
#[test]
fn record_row_loops_call_nothing_and_read_the_header_once() {
    let dir = setup(ROWS);
    let clif = stdout(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif",
            "--dump-filter",
            "total",
            "refs.plenty",
            "-o",
            "-",
        ],
    );
    // `i` is below `len(bodies)`, so not even a bounds check remains.
    assert!(!clif.contains("plenty_list_row"), "{clif}");
    assert!(!clif.contains("plenty_list_len"), "{clif}");
    assert!(!clif.contains(" call "), "{clif}");
    let asm = stdout(
        dir.path(),
        &[
            "--release",
            "--dump",
            "asm",
            "--dump-filter",
            "total",
            "refs.plenty",
            "-o",
            "-",
        ],
    );
    assert!(!asm.lines().any(support::native_call), "{asm}");
    // One read of the popped-row flag: `len(bodies)` and the cache share it.
    let removed = regex::Regex::new(r"movzb\S*\s+0x30\(").unwrap();
    assert_eq!(removed.find_iter(&asm).count(), 1, "{asm}");
}

/// Cached row references trap with the generic message and site.
#[rstest::rstest]
#[case("3", "index 3, length 3")]
#[case("-4", "index -4, length 3")]
#[case("9223372036854775807", "index 9223372036854775807, length 3")]
#[case("-9223372036854775808", "index -9223372036854775808, length 3")]
fn record_rows_in_loops_trap_out_of_bounds(#[case] index: &str, #[case] message: &str) {
    let source = format!(
        "@copy\nclass Body:\n    x: f64\n    n: i64\n\n\
         def poke(bodies: &mut list[Body], at: i64) -> f64:\n    mut sum = 0.0\n    for _ in range(2):\n        bodies[at].x += 1.0\n        sum += bodies[at].x\n    sum\n\n\
         def main() -> Result[(), AllocError | IoError]:\n    mut bodies = [Body(1.0, 0), Body(2.0, 0), Body(3.0, 0)]?\n    print(poke(&mut bodies, -1))?\n    print(poke(&mut bodies, {index}))?\n    Ok(())\n"
    );
    let dir = setup(&source);
    let poke = ops(dir.path(), "poke");
    assert!(poke.contains("ElementRefView("), "{poke}");
    assert!(!poke.contains("ElementRef("), "{poke}");
    for args in [
        &["--fast-compile", "refs.plenty"][..],
        &["--release", "refs.plenty"],
    ] {
        let output = plenty(dir.path(), args);
        assert!(!output.status.success(), "{source}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "9.0\n");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!("error: refs.plenty:9:9: index out of bounds: {message}\n")
        );
    }
}
