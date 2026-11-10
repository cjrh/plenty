//! Packed columns preserve scalar bits, owner loans and failed append inputs.
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

#[rstest]
#[case("constant")]
#[case("threshold")]
#[case("range")]
#[case("narrow")]
fn filter_count_does_not_read_past_guard_page(#[case] scenario: &str) {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/experiments/simd/clist/run.py"
        ))
        .arg(support::plenty_bin())
        .arg("--out")
        .arg(dir.path())
        .arg("--verify-only")
        .args(["--scenario", scenario])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[rstest]
#[case(false)]
#[case(true)]
fn filter_count_preserves_integer_comparisons_and_all_tails(#[case] release: bool) {
    // Every lane width, so lengths 0..130 cover each remainder of 4, 8 and
    // 16 lanes; each threshold sits at the sign boundary.
    let kinds = [
        ("signed", "i32", "-1i32"),
        ("unsigned", "u32", "2147483648u32"),
        ("signed16", "i16", "-1i16"),
        ("unsigned16", "u16", "32768u16"),
        ("signed8", "i8", "-1i8"),
        ("unsigned8", "u8", "128u8"),
    ];
    let mut source = String::from("class Row:\n    pad: bool\n");
    for (kind, ty, _) in kinds {
        source.push_str(&format!("    {kind}: {ty}\n"));
    }
    source.push('\n');
    let comparisons = ["==", "!=", "<", "<=", ">", ">="];
    for (kind, ty, threshold) in kinds {
        for (i, comparison) in comparisons.iter().enumerate() {
            source.push_str(&format!(
                "def scan_{kind}_{i}(rows: &clist[Row]) -> i64:\n    rows.column(Row.{kind}).filter(def(x: &{ty}) -> bool: *x {comparison} {threshold}).count()\n"
            ));
        }
    }
    // One checker per field keeps each function under the local-slot limit.
    for (kind, _, threshold) in kinds {
        source.push_str(&format!(
            "def check_{kind}(rows: &clist[Row]) -> Result[(), OverflowError[i64]]:\n"
        ));
        for (i, comparison) in comparisons.iter().enumerate() {
            source.push_str(&format!(
                "    mut expected_{i} = 0\n    for n in range(len(rows)):\n        if rows[n].{kind} {comparison} {threshold}:\n            expected_{i} += 1\n    check(scan_{kind}_{i}(rows) == expected_{i}, \"comparison {kind} {comparison}\").unwrap()\n"
            ));
        }
        source.push_str("    Ok(())\n");
    }
    source.push_str("for length in range(130):\n    mut rows = clist[Row]()\n    rows.reserve(length).unwrap()\n    for n in range(length):\n        bits = u32(n).wrapping_mul(2147483647u32)\n        rows.append(Row(True, i32.wrapping_from(bits), bits, i16.wrapping_from(bits), u16.wrapping_from(bits), i8.wrapping_from(bits), u8.wrapping_from(bits))).unwrap()\n");
    for (kind, _, _) in kinds {
        source.push_str(&format!("    check_{kind}(&rows)?\n"));
    }
    source.push_str("print(\"ok\").unwrap()\n");
    let source = support::program(&source).replacen(
        "def main() -> ():",
        "def main() -> Result[(), OverflowError[i64]]:",
        1,
    );
    let source = format!("{source}    Ok(())\n");
    let output = run_mode(&source, release);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"ok\n");
}

#[test]
fn filter_count_keeps_effects_and_nonmatching_chains_scalar() {
    let source = r#"
class Row:
    x: i32
def noisy(x: &i32) -> bool:
    print(*x).unwrap()
    *x > 2i32
mut rows = clist[Row]()
for n in range(7):
    rows.append(Row(i32(n))).unwrap()
check(rows.column(Row.x).filter(noisy).count() == 4, "side effects").unwrap()
limit = 2i32
check(rows.column(Row.x).filter(def [limit](x: &i32) -> bool: *x > limit).count() == 4, "capture").unwrap()
check(rows.column(Row.x).filter(def(x: &i32) -> bool: *x > 2i32).take(2).count() == 2, "take").unwrap()
check(rows.column(Row.x).skip(2).filter(def(x: &i32) -> bool: *x > 2i32).count() == 4, "skip").unwrap()
check(rows.column(Row.x).filter(def(x: &i32) -> bool: *x > 2i32).filter(def(x: &i32) -> bool: *x < 5i32).count() == 2, "two filters").unwrap()
print("ok").unwrap()
"#;
    let output = run_mode(source, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"0\n1\n2\n3\n4\n5\n6\nok\n");
}

#[rstest]
#[case(false)]
#[case(true)]
fn filter_count_runtime_thresholds_and_ranges(#[case] release: bool) {
    let fields = [
        (
            "signed",
            "i32",
            ["i32.MIN", "-1i32", "0i32", "1i32", "i32.MAX"],
        ),
        (
            "unsigned",
            "u32",
            ["0u32", "1u32", "2147483647u32", "2147483648u32", "u32.MAX"],
        ),
        (
            "signed16",
            "i16",
            ["i16.MIN", "-1i16", "0i16", "1i16", "i16.MAX"],
        ),
        (
            "unsigned16",
            "u16",
            ["0u16", "1u16", "32767u16", "32768u16", "u16.MAX"],
        ),
        ("signed8", "i8", ["i8.MIN", "-1i8", "0i8", "1i8", "i8.MAX"]),
        (
            "unsigned8",
            "u8",
            ["0u8", "1u8", "127u8", "128u8", "u8.MAX"],
        ),
    ];
    let mut source = String::from("class Row:\n");
    for (field, ty, _) in fields {
        source.push_str(&format!("    {field}: {ty}\n"));
    }
    let predicates = [
        "*x == low",
        "*x != low",
        "*x < low",
        "*x <= low",
        "*x > low",
        "*x >= low",
        "low < *x and *x < high",
        "low <= *x and *x < high",
        "*x > low and high >= *x",
        "*x >= low and *x <= high",
    ];
    for (field, ty, _) in fields {
        for (i, predicate) in predicates.iter().enumerate() {
            source.push_str(&format!(
                "def scan_{field}_{i}(rows: &clist[Row], low: {ty}, high: {ty}) -> i64:\n    rows.column(Row.{field}).filter(def [low, high](x: &{ty}) -> bool: {predicate}).count()\n"
            ));
        }
    }
    for (field, ty, _) in fields {
        source.push_str(&format!(
            "def check_{field}(rows: &clist[Row], low: {ty}, high: {ty}) -> Result[(), OverflowError[i64]]:\n"
        ));
        for (i, predicate) in predicates.iter().enumerate() {
            let expected = predicate.replace("*x", &format!("rows[n].{field}"));
            source.push_str(&format!(
                "    mut expected_{i} = 0\n    for n in range(len(rows)):\n        if {expected}:\n            expected_{i} += 1\n    check(scan_{field}_{i}(rows, low, high) == expected_{i}, \"{field} {i}\").unwrap()\n"
            ));
        }
        source.push_str("    Ok(())\n");
    }
    // The narrow fields wrap these to their own minimum, maximum and sign
    // boundary; 34 rows exceed two 16-lane vectors.
    source.push_str("endpoints = [0u32, 1u32, 2147483647u32, 2147483648u32, u32.MAX, 2u32, 4294967294u32, 127u32, 128u32, 32767u32, 32768u32].unwrap()\nfor length in range(34):\n    mut rows = clist[Row]()\n    for n in range(length):\n        bits = endpoints[n % len(endpoints)]\n        rows.append(Row(i32.wrapping_from(bits), bits, i16.wrapping_from(bits), u16.wrapping_from(bits), i8.wrapping_from(bits), u8.wrapping_from(bits))).unwrap()\n");
    for (field, _, bounds) in fields {
        source.push_str(&format!("    for low in [{0}, {1}, {2}, {3}, {4}].unwrap():\n        for high in [{0}, {1}, {2}, {3}, {4}].unwrap():\n            check_{field}(&rows, low, high)?\n", bounds[0], bounds[1], bounds[2], bounds[3], bounds[4]));
    }
    source.push_str("print(\"ok\").unwrap()\n");
    let source = support::program(&source).replacen(
        "def main() -> ():",
        "def main() -> Result[(), OverflowError[i64] | ZeroDivisionError]:",
        1,
    );
    let source = format!("{source}    Ok(())\n");
    let output = run_mode(&source, release);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"ok\n");
}

#[test]
fn filter_count_preserves_capture_snapshot_and_short_circuit_effects() {
    let source = r#"
class Row:
    x: i32
def scan(rows: &clist[Row], low: i32, high: i32) -> i64:
    mut original = low
    predicate = def [original, high](x: &i32) -> bool:
        *x >= original and *x < high
    original = 100i32
    rows.column(Row.x).filter(predicate).count()
def effect(x: &i32) -> bool:
    print(*x).unwrap()
    True
mut rows = clist[Row]()
for n in range(9):
    rows.append(Row(i32(n))).unwrap()
check(scan(&rows, 2i32, 7i32) == 5, "snapshot").unwrap()
check(scan(&rows, 3i32, 8i32) == 5, "new snapshot on another call").unwrap()
check(rows.column(Row.x).filter(def(x: &i32) -> bool: *x > 6i32 and effect(x)).count() == 2, "short circuit").unwrap()
skipped = def(x: &i32) -> bool:
    if *x < 0i32:
        # The guard proves x + MAX is in -1..MAX-1; retain short circuiting.
        shifted = unchecked:
            *x + i32.MAX
        return shifted > 0i32
    False
check(rows.column(Row.x).filter(skipped).count() == 0, "skipped overflow").unwrap()
print("ok").unwrap()
"#;
    let output = run_mode(source, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"7\n8\nok\n");
}

#[test]
fn filter_count_emits_vectors_only_for_the_bounded_predicate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scan.plenty");
    std::fs::write(
        &path,
        r#"
class Row:
    x: i32
def eligible(rows: &clist[Row]) -> i64:
    rows.column(Row.x).filter(def(x: &i32) -> bool: *x > 5i32).count()
def predicate(x: &i32) -> bool:
    *x > 5i32
def named(rows: &clist[Row]) -> i64:
    rows.column(Row.x).filter(predicate).count()
def arithmetic_predicate(x: &i32) -> bool:
    # Deliberate trapping IR subject: keep the original arithmetic predicate
    # and infallible filter ABI. Main supplies an empty column, not test data.
    unchecked:
        *x + 1i32 > 5i32
def checked(rows: &clist[Row]) -> i64:
    rows.column(Row.x).filter(arithmetic_predicate).count()
def bound_range_control(rows: &clist[Row], low: i32) -> i64:
    predicate = def [low](x: &i32) -> bool:
        *x > low and *x < 5i32
    rows.column(Row.x).filter(predicate).count()
def runtime(rows: &clist[Row], limit: i32) -> i64:
    rows.column(Row.x).filter(def [limit](x: &i32) -> bool: limit < *x).count()
def interval(rows: &clist[Row], low: i32, high: i32) -> i64:
    rows.column(Row.x).filter(def [low, high](x: &i32) -> bool: low <= *x and *x < high).count()
def constant_range(x: &i32) -> bool:
    *x >= 1i32 and *x < 4i32
def named_range(rows: &clist[Row]) -> i64:
    rows.column(Row.x).filter(constant_range).count()
def mixed_range(rows: &clist[Row], low: i32) -> i64:
    rows.column(Row.x).filter(def [low](x: &i32) -> bool: *x >= low and *x < 4i32).count()
def borrowed(rows: &clist[Row], limit: i32) -> i64:
    rows.column(Row.x).filter(def [&limit](x: &i32) -> bool: *x > limit).count()
def checked_range(rows: &clist[Row], low: i32) -> i64:
    predicate = def [low](x: &i32) -> bool:
        # Deliberate trapping IR subject: preserve the original short-circuit
        # arithmetic predicate/filter ABI. Main supplies an empty column.
        unchecked:
            *x > low and *x + 1i32 < 5i32
    rows.column(Row.x).filter(predicate).count()
def disjunction(rows: &clist[Row], low: i32) -> i64:
    rows.column(Row.x).filter(def [low](x: &i32) -> bool: *x < low or *x > 5i32).count()
def main() -> Result[(), IoError]:
    rows = clist[Row]()
    drop(bound_range_control(&rows, 2i32))
    print(eligible(&rows), named(&rows), checked(&rows))?
    print(runtime(&rows, 2i32), interval(&rows, 1i32, 4i32), borrowed(&rows, 2i32), checked_range(&rows, 2i32), disjunction(&rows, 2i32))?
    print(named_range(&rows), mixed_range(&rows, 1i32))?
    Ok(())
"#,
    )
    .unwrap();
    for (function, vectorized) in [
        ("eligible", true),
        ("named", true),
        ("checked", false),
        ("bound_range_control", true),
        ("runtime", true),
        ("interval", true),
        ("named_range", true),
        ("mixed_range", true),
        ("borrowed", false),
        ("checked_range", false),
        ("disjunction", false),
    ] {
        let output = std::process::Command::new(support::plenty_bin())
            .args(["--release", "--dump", "clif,asm", "--dump-filter", function])
            .arg(&path)
            .args(["-o", "-"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.contains("load.i32x4"), vectorized, "{text}");
        if vectorized {
            let comparisons: &[&str] = if cfg!(target_arch = "aarch64") {
                &["cmgt ", "cmge ", "cmeq "]
            } else {
                &["pcmpgtd"]
            };
            assert!(
                comparisons.iter().any(|mnemonic| text.contains(mnemonic)),
                "{text}"
            );
        }
    }
}

/// 8- and 16-bit lane counters are widened before they can wrap: after 255
/// vectors of bytes and 65,535 of words. Every row matches, so each lane
/// counter reaches its maximum; lengths one row and one vector either side
/// of each chunk make an off-by-one chunk length wrap and miscount.
#[test]
fn filter_count_widens_narrow_lane_counters_before_they_wrap() {
    let source = r#"
class Row:
    byte: u8
    word: i16
bytes = 255 * 16
words = 65535 * 8
lengths = [bytes - 16, bytes - 1, bytes, bytes + 1, bytes + 16, words - 8, words - 1, words, words + 1, words + 8].unwrap()
mut rows = clist[Row]()
rows.reserve(words + 8).unwrap()
for length in lengths:
    while len(rows) < length:
        rows.append(Row(255u8, i16.MIN)).unwrap()
    check(rows.column(Row.byte).filter(def(x: &u8) -> bool: *x == 255u8).count() == length, "bytes").unwrap()
    check(rows.column(Row.word).filter(def(x: &i16) -> bool: *x != 5i16).count() == length, "words").unwrap()
print("ok").unwrap()
"#;
    let source = support::program(source).replacen(
        "def main() -> ():",
        "def main() -> Result[(), OverflowError[i64]]:",
        1,
    );
    let source = format!("{source}    Ok(())\n");
    let output = run_mode(&source, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"ok\n");
}

#[test]
fn filter_count_emits_narrow_vectors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scan.plenty");
    std::fs::write(
        &path,
        r#"
class Row:
    a: u8
    b: i8
    c: u16
    d: i16
def unsigned_bytes(rows: &clist[Row]) -> i64:
    rows.column(Row.a).filter(def(x: &u8) -> bool: *x > 127u8).count()
def signed_bytes(rows: &clist[Row], low: i8) -> i64:
    rows.column(Row.b).filter(def [low](x: &i8) -> bool: low < *x and *x < 3i8).count()
def words(rows: &clist[Row], limit: u16) -> i64:
    rows.column(Row.c).filter(def [limit](x: &u16) -> bool: *x > limit).count()
def signed_words(rows: &clist[Row]) -> i64:
    rows.column(Row.d).filter(def(x: &i16) -> bool: *x < -5i16).count()
def main() -> Result[(), IoError]:
    rows = clist[Row]()
    print(unsigned_bytes(&rows), signed_bytes(&rows, 1i8), words(&rows, 2u16), signed_words(&rows))?
    Ok(())
"#,
    )
    .unwrap();
    for (function, load, x64) in [
        ("unsigned_bytes", "load.i8x16", "pcmpgtb"),
        ("signed_bytes", "load.i8x16", "pcmpgtb"),
        ("words", "load.i16x8", "pcmpgtw"),
        ("signed_words", "load.i16x8", "pcmpgtw"),
    ] {
        let output = std::process::Command::new(support::plenty_bin())
            .args(["--release", "--dump", "clif,asm", "--dump-filter", function])
            .arg(&path)
            .args(["-o", "-"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(load), "{function}: {text}");
        let comparison = if cfg!(target_arch = "aarch64") {
            "cmgt "
        } else {
            x64
        };
        assert!(text.contains(comparison), "{function}: {text}");
    }
}

fn run_mode(source: &str, release: bool) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clist.plenty");
    std::fs::write(
        &path,
        format!("from std.test import check\n{}", support::program(source)),
    )
    .unwrap();
    let mut command = std::process::Command::new(support::plenty_bin());
    command.current_dir(dir.path()).env("PLENTY_CACHE", "off");
    if !release {
        command.arg("--fast-compile");
    }
    command.arg(path).output().unwrap()
}

#[rstest]
#[case(false)]
#[case(true)]
fn direct_access_survives_growth_and_copies_stay_independent(#[case] release: bool) {
    let output = run_mode(
        r#"class Values:
    flag: bool
    a: i8
    b: u8
    c: i16
    d: u16
    e: i32
    f: u32
    g: i64
    h: u64
    i: f32
    j: f64
def gather(rows: &clist[Values], index: i64) -> Option[Values]:
    rows.copy_row(index)
def main() -> Result[(), OverflowError[i64]]:
    mut rows = clist[Values]()
    for n in range(17):
        rows.append(Values(False, 0i8, 0u8, 0i16, 0u16, 0i32, 0u32, 0, 0u64, 0.0f32, 0.0)).unwrap()
        rows[-1].flag = True
        rows[-1].a = -128i8
        rows[-1].b = 255u8
        rows[-1].c = -32768i16
        rows[-1].d = 65535u16
        rows[-1].e = -2147483648i32
        rows[-1].f = 4294967295u32
        rows[-1].g = -9223372036854775808
        rows[-1].h = 18446744073709551615u64
        rows[-1].i = -0.0f32
        rows[-1].j = -0.0
        check(rows[n].flag and rows[n].a == -128i8 and rows[n].b == 255u8, "byte fields").unwrap()
        check(rows[n].c == -32768i16 and rows[n].d == 65535u16, "16-bit fields").unwrap()
        check(rows[n].e == -2147483648i32 and rows[n].f == 4294967295u32, "32-bit fields").unwrap()
        check(rows[n].g == -9223372036854775808 and rows[n].h == 18446744073709551615u64, "64-bit fields").unwrap()
    saved = gather(&rows, -17).unwrap()
    for n in range(17):
        check(rows.copy_row(n).unwrap() == rows.copy_row(n - 17).unwrap(), "negative indices").unwrap()
    rows.reserve(100).unwrap()
    rows[0].a = 1i8
    check(saved.a == -128i8 and rows[0].a == 1i8, "independent copy").unwrap()
    check(rows.copy_row(-18).is_none() and rows.copy_row(17).is_none(), "bounds").unwrap()
    check(rows.copy_row(i64.MIN).is_none() and rows.copy_row(i64.MAX).is_none(), "extreme indices").unwrap()
    print(saved.i, saved.j, rows[-1].i, rows[-1].j).unwrap()
    drop(rows)
    print(saved.h).unwrap()
    Ok(())
"#,
        release,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "-0.0 -0.0 -0.0 -0.0\n18446744073709551615\n"
    );
}

#[rstest]
#[case(false)]
#[case(true)]
fn indexed_access_keeps_bounds_traps(#[case] release: bool) {
    for (setup, index) in [
        ("", "0"),
        ("rows.reserve(8).unwrap()", "-1"),
        ("rows.append(Point(1)).unwrap()", "1"),
        ("rows.append(Point(1)).unwrap()", "-2"),
        ("rows.append(Point(1)).unwrap()", "i64.MIN"),
        ("rows.append(Point(1)).unwrap()", "i64.MAX"),
    ] {
        for access in [
            format!("print(rows[{index}].x).unwrap()"),
            format!("rows[{index}].x = 9"),
        ] {
            let output = run_mode(
                &format!(
                    "class Point:\n    x: i64\nmut rows = clist[Point]()\n{setup}\n{access}\n"
                ),
                release,
            );
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(!output.status.success(), "{setup}: {access}");
            assert!(
                error.contains("clist.plenty:") && error.contains("clist index out of bounds"),
                "{error}"
            );
        }
    }
}

#[test]
fn row_reconstruction_has_no_generic_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gather.plenty");
    std::fs::write(
        &path,
        support::program(
            r#"
class Point:
    x: i64
    y: f32
def gather(rows: &clist[Point], index: i64) -> Option[Point]:
    rows.copy_row(index)
mut rows = clist[Point]()
rows.append(Point(1, 2.0f32)).unwrap()
print(gather(&rows, 0)).unwrap()
"#,
        ),
    )
    .unwrap();
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args(["--release", "--dump", "asm", "--dump-filter", "gather"])
        .arg(path)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let assembly = String::from_utf8(output.stdout).unwrap();
    assert!(assembly.contains("gather"), "{assembly}");
    assert!(!assembly.contains("plenty_op_"), "{assembly}");
}

#[test]
fn nested_owner_indices_are_evaluated_in_order_and_only_once() {
    native(
        r#"class Point:
    x: i64
def outer() -> i64:
    print("owner").unwrap()
    0
def inner() -> i64:
    print("row").unwrap()
    0
def rhs() -> i64:
    print("rhs").unwrap()
    3
def main() -> Result[(), OverflowError[i64]]:
    mut rows = clist[Point]()
    rows.append(Point(4)).unwrap()
    mut groups = [rows].unwrap()
    print(groups[outer()][inner()].x).unwrap()
    print(groups[outer()].copy_row(inner()).unwrap().x).unwrap()
    groups[outer()][inner()].x = rhs()
    groups[outer()][inner()].x += rhs()
    print(groups[0][0].x).unwrap()
    mut wrapped = (clist[Point](), 7)
    wrapped[0].append(Point(8)).unwrap()
    wrapped[0][0].x += 2
    print(wrapped[0][0].x).unwrap()
    shared = Shared(groups).unwrap()
    print(shared[outer()][inner()].x).unwrap()
    Ok(())
"#,
        "owner\nrow\n4\nowner\nrow\n4\nrhs\nowner\nrow\nowner\nrow\nrhs\n6\n10\nowner\nrow\n6",
    );
}

#[test]
fn primitive_columns_rows_and_pipelines() {
    native(
        r#"class Point:
    x: i64
    y: f64
def add(total: i64, x: i64) -> Result[i64, OverflowError[i64]]:
    Ok(total + x)
def main() -> Result[(), OverflowError[i64]]:
    mut points = clist[Point]()
    print(len(points), points.capacity()).unwrap()
    points.reserve(10).unwrap()
    for i in range(10):
        points.append(Point(i, f64(i))).unwrap()
    points[-1].x = 40
    print(len(points), points[0].x, points[-1].x, points[-2].y).unwrap()
    print(points.copy_row(2), points.copy_row(-1), points.copy_row(10)).unwrap()
    print(iter(points.column(Point.x)).try_fold(0, add)?).unwrap()
    mut total = 0
    for x, y in zip(points.column(Point.x), points.column(Point.y)):
        total += x + i64(y)
    print(total).unwrap()
    Ok(())
"#,
        "0 0\n10 0 40 8.0\nOption[Point].Some(Point(x=2, y=2.0)) Option[Point].Some(Point(x=40, y=9.0)) Option[Point].Nothing\n76\n121",
    );
}

#[test]
fn packed_widths_and_float_bits_roundtrip() {
    native(
        r#"
class Values:
    flag: bool
    a: i8
    b: u8
    c: i16
    d: u16
    e: i32
    f: u32
    g: i64
    h: u64
    i: f32
    j: f64
mut rows = clist[Values].with_capacity(1).unwrap()
rows.append(Values(True, -128i8, 255u8, -32768i16, 65535u16, -2147483648i32, 4294967295u32, -9223372036854775808, 18446744073709551615u64, -0.0f32, -0.0)).unwrap()
print(rows.copy_row(0)).unwrap()
for x in rows.column(Values.i):
    print(x).unwrap()
for x in rows.column(Values.h):
    print(x).unwrap()
for x in rows.column(Values.a):
    print(x).unwrap()
"#,
        "Option[Values].Some(Values(flag=True, a=-128, b=255, c=-32768, d=65535, e=-2147483648, f=4294967295, g=-9223372036854775808, h=18446744073709551615, i=-0.0, j=-0.0))\n-0.0\n18446744073709551615\n-128",
    );
}

#[rstest]
#[case("items.append(Point(3)).unwrap()", "borrow")]
#[case("items.reserve(100).unwrap()", "borrow")]
#[case("drop(items.remove(0))", "borrow")]
#[case("items.clear()", "borrow")]
#[case("items[0].x = 10", "borrow")]
#[case("moved = items", "borrow")]
fn column_plan_protects_the_whole_owner(#[case] mutation: &str, #[case] expected: &str) {
    for source in [
        "items.column(Point.x)",
        "items.column(Point.x).zip(items.column(Point.x))",
    ] {
        let error = support::check_source(&format!(
            r#"
class Point:
    x: i64
mut items = clist[Point]()
items.append(Point(1)).unwrap()
values = {source}
{mutation}
print(values.count()).unwrap()
"#
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[rstest]
#[case("class Point:\n    x: str", "primitive scalar")]
#[case(
    "class Point:\n    x: i64\n    def __del__(self: &mut Point) -> ():\n        ()",
    "destructor"
)]
fn unsupported_rows_are_diagnosed(#[case] schema: &str, #[case] expected: &str) {
    let error = support::check_source(&format!("{schema}\nitems = clist[Point]()"))
        .unwrap_err()
        .to_string();
    assert!(error.contains(expected), "{error}");
}

#[test]
fn empty_and_out_of_bounds_rows() {
    native(
        r#"
class Point:
    x: i64
items = clist[Point]()
print(items.copy_row(0), items.copy_row(-1), items.column(Point.x).count()).unwrap()
"#,
        "Option[Point].Nothing Option[Point].Nothing 0",
    );
    let output = support::run(
        "class Point:\n    x: i64\nitems = clist[Point]()\nprint(items[0].x).unwrap()",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("clist index out of bounds"));
}

#[test]
fn projected_enum_owner_preserves_tags_and_row_updates_evaluate_once() {
    native(
        r#"class Point:
    x: i64
def build() -> clist[Point]:
    clist[Point]()
def grow(items: &mut clist[Point]) -> ():
    items.append(Point(7)).unwrap()
def position() -> i64:
    print("index").unwrap()
    0
def increment() -> i64:
    print("value").unwrap()
    2
def main() -> Result[(), OverflowError[i64]]:
    mut wrapped = Some(build())
    match &mut wrapped:
        case Some(items):
            grow(&mut *items)
        case Nothing:
            ()
    match wrapped:
        case Some(items):
            mut owned = items
            owned[position()].x += increment()
            owned[position()].x = increment()
            print(owned[0].x).unwrap()
        case Nothing:
            print("lost tag").unwrap()
    Ok(())
"#,
        "index\nvalue\nvalue\nindex\n2",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_append_preserves_row_and_reserved_work_does_not_allocate() {
    native(
        r#"
class Point:
    x: i64
    y: i64
print("__test_fail_allocations_after_0__").unwrap()
mut items = clist[Point]()
failed = items.append(Point(21, 42))
print("__test_restore_allocations__").unwrap()
print(len(items)).unwrap()
match failed:
    case Err(AppendError.OutOfMemory(row)):
        print(row).unwrap()
        items.reserve(1).unwrap()
        print("__test_fail_allocations_after_0__").unwrap()
        items.append(row).unwrap()
        copied = items.copy_row(0)
        count = items.column(Point.x).count()
        print("__test_restore_allocations__").unwrap()
        print(copied, count).unwrap()
    case _:
        print("wrong outcome").unwrap()
"#,
        "0\nPoint(x=21, y=42)\nOption[Point].Some(Point(x=21, y=42)) 1",
    );
}

#[test]
fn column_plans_cannot_escape_into_generator_storage() {
    let error = support::check_source(
        r#"
class Point:
    x: i64
items = clist[Point]()
escaped = items.column(Point.x).generator()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("borrowed state cannot be suspended"),
        "{error}"
    );
}

// B76: relax this conservative boundary only with generator-frame loan support.
#[test]
fn column_iteration_in_generator_functions_is_explicitly_rejected() {
    let error = support::check_source(
        r#"
class Point:
    x: i64
def values() -> Generator[i64]:
    items = clist[Point]()
    for value in items.column(Point.x):
        yield value
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("column iteration is not supported inside generator functions"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_growth_and_overflow_leave_existing_columns_unchanged() {
    native(
        r#"
class Point:
    x: i64
    y: u8
mut items = clist[Point].with_capacity(8).unwrap()
for i in range(8):
    items.append(Point(i, u8(i))).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
failed = items.append(Point(8, 255u8))
print("__test_restore_allocations__").unwrap()
print(len(items), items.capacity(), items[-1].x, items[-1].y).unwrap()
print(str.repr(items.reserve(-1)).unwrap(), str.repr(items.reserve(9223372036854775807)).unwrap()).unwrap()
match failed:
    case Err(AppendError.OutOfMemory(row)):
        items.append(row).unwrap()
    case _:
        print("wrong outcome").unwrap()
print(items.copy_row(8), len(items)).unwrap()
"#,
        "8 8 7 7\nResult[(), AllocError].Err(AllocError.CapacityOverflow) Result[(), AllocError].Err(AllocError.CapacityOverflow)\nOption[Point].Some(Point(x=8, y=255)) 9",
    );
}

#[test]
fn generic_rows_and_stored_owners_keep_type_identity() {
    native(
        r#"
class Cell[T]:
    value: T
class Store:
    rows: clist[Cell[i64]]
def count[T](rows: &clist[T]) -> i64:
    len(rows)
mut store = Store(clist[Cell[i64]]())
store.rows.append(Cell[i64](7)).unwrap()
print(count(&store.rows), store.rows.copy_row(0).unwrap().value).unwrap()
owners = [store].unwrap()
print(len(owners)).unwrap()
"#,
        "1 7\n1",
    );
}

#[test]
fn column_selectors_and_copy_rows_obey_module_privacy() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("model.plenty"),
        "pub class Point:\n    x: i64\npub def make() -> clist[Point]:\n    clist[Point]()\n",
    )
    .unwrap();
    for operation in [
        "clist[Point]()",
        "items.copy_row(0)",
        "items.remove(0)",
        "items.column(Point.x).count()",
        "items[0].x",
    ] {
        std::fs::write(dir.path().join("main.plenty"), format!("from model import Point, make\ndef main() -> ():\n    items = make()\n    value = {operation}\n")).unwrap();
        let error = plenty::check_file(&dir.path().join("main.plenty"), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("private"), "{operation}: {error}");
    }
}

#[test]
fn clist_aliases_equality_and_nested_nan_comparisons() {
    native(
        r#"
class Point:
    x: f64
type Points = clist[Point]
class Holder:
    points: Points
mut a = Points()
mut b = Points.new()
print(a == b).unwrap()
a.append(Point(1.0)).unwrap()
b.append(Point(1.0)).unwrap()
print(a == b).unwrap()
a[0].x = f64.NAN
print(a == a).unwrap()
holder = Holder(a)
print(holder == holder).unwrap()
"#,
        "True\nTrue\nFalse\nFalse",
    );
}

#[rstest]
#[case("&items[0].x", "packed clist fields cannot be borrowed")]
#[case("items[0]", "rows are not contiguous")]
#[case("items.column(Point.y)", "unknown field")]
fn unsupported_projection_diagnostics(#[case] expression: &str, #[case] message: &str) {
    let error = support::check_source(&format!(
        "class Point:\n    x: i64\nitems = clist[Point]()\nvalue = {expression}"
    ))
    .unwrap_err()
    .to_string();
    assert!(error.contains(message), "{error}");
}

#[rstest]
#[case(false)]
#[case(true)]
fn ordered_removal_preserves_columns_bounds_and_owned_rows(#[case] release: bool) {
    let output = run_mode(
        r#"class Row:
    flag: bool
    a: i8
    b: u8
    c: i16
    d: u16
    e: i32
    f: u32
    g: i64
    h: u64
    i: f32
    j: f64
def row(n: i64) -> Result[Row, OverflowError[i64] | ZeroDivisionError]:
    Ok(Row(n % 2 == 0, i8(-n), u8(n), i16(-n), u16(n), i32(-n), u32(n), -n, u64(n), f32(n), f64(n)))
def main() -> Result[(), OverflowError[i64] | ZeroDivisionError]:
    for length in range(1, 34):
        for removed in range(length):
            mut rows = clist[Row].with_capacity(length).unwrap()
            for n in range(length):
                rows.append(row(n)?).unwrap()
            capacity = rows.capacity()
            check(rows.remove(length).is_none(), "past end").unwrap()
            check(rows.remove(-length - 1).is_none(), "before start").unwrap()
            check(rows.remove(i64.MIN).is_none() and rows.remove(i64.MAX).is_none(), "extreme bounds").unwrap()
            check(len(rows) == length, "invalid removal keeps length").unwrap()
            index = removed if removed % 2 == 0 else removed - length
            saved = rows.remove(index).unwrap()
            check(saved == row(removed)?, "removed row").unwrap()
            check(len(rows) == length - 1 and rows.capacity() == capacity, "size and capacity").unwrap()
            for n in range(len(rows)):
                expected = n if n < removed else n + 1
                check(rows.copy_row(n).unwrap() == row(expected)?, "all columns stay aligned").unwrap()
            rows.append(row(99)?).unwrap()
            check(rows.copy_row(-1).unwrap() == row(99)?, "append uses reclaimed tail").unwrap()
            check(rows.column(Row.a).count() == length, "scan has live length").unwrap()
            rows.clear()
            check(len(rows) == 0 and rows.capacity() == capacity, "clear retains capacity").unwrap()
            check(rows.column(Row.h).count() == 0, "clear empties scan").unwrap()
            check(rows.remove(0).is_none() and rows.remove(-1).is_none(), "allocated empty removal").unwrap()
            rows.clear()
            rows.append(row(100)?).unwrap()
            check(rows.copy_row(0).unwrap() == row(100)?, "append after clear").unwrap()
            drop(rows)
            check(saved == row(removed)?, "returned row outlives storage").unwrap()
    print("ok").unwrap()
    Ok(())
"#,
        release,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"ok\n");
}

#[test]
fn remove_and_clear_projected_owners_preserve_tags_and_evaluate_indices_once() {
    native(
        r#"
class Point:
    x: i64
def owner_index() -> i64:
    print("owner").unwrap()
    0
def row_index() -> i64:
    print("row").unwrap()
    -1
def clear(rows: &mut clist[Point]) -> ():
    rows.clear()
mut wrapped = Some(clist[Point]())
match &mut wrapped:
    case Some(rows):
        rows.append(Point(7)).unwrap()
        print(rows.remove(0)).unwrap()
        clear(&mut *rows)
    case Nothing:
        ()
match wrapped:
    case Some(rows):
        mut groups = [rows].unwrap()
        groups[0].append(Point(8)).unwrap()
        print(groups[owner_index()].remove(row_index())).unwrap()
        groups[owner_index()].clear()
        print(len(groups[0])).unwrap()
    case Nothing:
        print("lost tag").unwrap()
"#,
        "Option[Point].Some(Point(x=7))\nrow\nowner\nOption[Point].Some(Point(x=8))\nowner\n0",
    );
}

#[rstest]
#[case("drop(items.remove(0))")]
#[case("items.clear()")]
fn remove_and_clear_require_mutable_access(#[case] operation: &str) {
    for declaration in [
        "items = clist[Point]()",
        "mut owner = clist[Point]()\nitems = &owner",
    ] {
        let error = support::check_source(&format!(
            "class Point:\n    x: i64\n{declaration}\n{operation}"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("mutab"), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn remove_clear_and_reuse_do_not_allocate_and_preserve_float_bits() {
    native(
        r#"
class Row:
    small: i8
    unsigned: u64
    x: f32
    y: f64
mut rows = clist[Row].with_capacity(2).unwrap()
rows.append(Row(-128i8, u64.MAX, -0.0f32, f64.NAN)).unwrap()
rows.append(Row(127i8, 0u64, f32.NAN, -0.0)).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
mut empty = clist[Row]()
empty.clear()
absent = empty.remove(0)
first = rows.remove(0).unwrap()
second = rows.remove(-1).unwrap()
rows.clear()
rows.append(Row(first.small, first.unsigned, first.x, first.y)).unwrap()
rows.clear()
rows.append(Row(second.small, second.unsigned, second.x, second.y)).unwrap()
print("__test_restore_allocations__").unwrap()
print(absent.is_none(), len(empty), empty.capacity()).unwrap()
print(first.small, first.unsigned, first.x, first.y != first.y).unwrap()
print(rows[0].small, rows[0].unsigned, rows[0].x != rows[0].x, rows[0].y).unwrap()
"#,
        "True 0 0\n-128 18446744073709551615 -0.0 True\n127 0 True -0.0",
    );
}

/// Field access on clist rows in loops (issue #759): each loop reads a row
/// count and column address once per accessed field, shares one bounds check
/// among the fields of a row, and reads the header at each access when the
/// loop can append, reserve, remove, clear or pass the clist on.
const LOOPS: &str = r#"class Body:
    x: f64
    v: f64
    n: i64


def step(bodies: &mut clist[Body], dt: f64) -> Result[(), OverflowError[i64]]:
    for i in range(len(bodies)):
        bodies[i].x += dt * bodies[i].v
        for j in range(i + 1, len(bodies)):
            bodies[j].v -= bodies[i].x
            bodies[i].n += 1
    Ok(())


def total(bodies: &clist[Body]) -> f64:
    mut sum = 0.0
    for i in range(len(bodies)):
        sum += bodies[i].x + bodies[i].v
    sum


def poke(bodies: &mut clist[Body], at: i64) -> f64:
    mut sum = 0.0
    for _ in range(2):
        bodies[at].x += 1.0
        sum += bodies[at].x + bodies[at].v
    sum


def touch(bodies: &mut clist[Body]) -> Result[(), AllocError | AppendError[Body]]:
    bodies.append(Body(0.0, 0.0, -1))?
    Ok(())


def grow(bodies: &mut clist[Body]) -> Result[(), AllocError | AppendError[Body] | OverflowError[i64]]:
    for i in range(2):
        bodies[i].n += 10
        bodies.append(Body(1.0, 1.0, i))?
    Ok(())


def call(bodies: &mut clist[Body]) -> Result[(), AllocError | AppendError[Body] | OverflowError[i64]]:
    for i in range(2):
        bodies[i].n += 100
        touch(bodies)?
    Ok(())


def shrink(bodies: &mut clist[Body]) -> Result[(), OverflowError[i64]]:
    for _ in range(2):
        bodies[0].n += 1000
        bodies.remove(0)
    Ok(())


def main() -> Result[(), AllocError | AppendError[Body] | IoError | OverflowError[i64]]:
    mut bodies = clist[Body]()
    for k in range(3):
        bodies.append(Body(f64(2 * k + 1), f64(2 * k + 2), 0))?
    step(&mut bodies, 0.5)?
    print(f"{total(&bodies)} {bodies[0].n} {bodies[1].n} {bodies[2].n}"?)?
    print(poke(&mut bodies, -1))?
    grow(&mut bodies)?
    call(&mut bodies)?
    print(f"{len(bodies)} {bodies[0].n} {bodies[1].n} {bodies[4].n} {bodies[6].n}"?)?
    shrink(&mut bodies)?
    print(f"{len(bodies)} {bodies[0].n}"?)?
    Ok(())
"#;

fn dump(dir: &std::path::Path, what: &str, function: &str) -> String {
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(["--release", "--dump", what, "--dump-filter", function])
        .args(["loops.plenty", "-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn loops_dir(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("loops.plenty"), source).unwrap();
    dir
}

#[rstest]
#[case(false)]
#[case(true)]
fn row_fields_in_loops_keep_their_results(#[case] release: bool) {
    let dir = loops_dir(LOOPS);
    let mut command = std::process::Command::new(support::plenty_bin());
    command.current_dir(dir.path()).env("PLENTY_CACHE", "off");
    if !release {
        command.arg("--fast-compile");
    }
    let output = command.arg("loops.plenty").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "15.0 2 1 0\n13.0\n7 112 111 1 -1\n5 0\n"
    );
}

#[test]
fn row_fields_use_the_loop_cache_unless_the_loop_resizes() {
    let dir = loops_dir(LOOPS);
    let total = dump(dir.path(), "ops", "total");
    assert_eq!(total.matches("GetView(").count(), 2, "{total}");
    assert!(!total.contains("Get("), "{total}");
    // The outer loop's three accesses and the inner loop's five share one
    // cache (issue #943).
    let step = dump(dir.path(), "ops", "step");
    assert_eq!(
        step.matches("GetView(").count() + step.matches("SetView(").count(),
        8,
        "{step}"
    );
    assert!(!step.contains("Clist(Get("), "{step}");
    assert!(!step.contains("Clist(Set("), "{step}");
    for function in ["grow", "call", "shrink"] {
        let ops = dump(dir.path(), "ops", function);
        assert!(ops.contains("Clist(Get("), "{function}: {ops}");
        assert!(!ops.contains("View("), "{function}: {ops}");
    }
}

/// The cached loops call nothing on their hot paths: `total` needs no check,
/// and one cold trap call guards the three accesses to row `at` in `poke`.
#[test]
fn row_field_loops_call_nothing_and_check_each_row_once() {
    let dir = loops_dir(LOOPS);
    let total = dump(dir.path(), "clif", "total");
    assert!(!total.contains(" call "), "{total}");
    let asm = dump(dir.path(), "asm", "total");
    assert!(!asm.lines().any(support::native_call), "{asm}");
    let poke = dump(dir.path(), "clif", "poke");
    let calls: Vec<_> = poke
        .lines()
        .filter(|line| line.contains(" call "))
        .collect();
    assert_eq!(calls.len(), 1, "{poke}");
    assert!(poke.contains("; plenty_trap_clist_index"), "{poke}");
}

#[rstest]
#[case("2", false)]
#[case("-3", false)]
#[case("3", true)]
#[case("-4", true)]
#[case("i64.MAX", true)]
#[case("i64.MIN", true)]
fn row_fields_in_loops_trap_out_of_bounds(#[case] index: &str, #[case] traps: bool) {
    for (rows, at) in [(3, index), (0, "0")] {
        let source = format!(
            "class Body:\n    x: f64\n\n\
             def poke(bodies: &mut clist[Body], at: i64) -> f64:\n    mut sum = 0.0\n    for _ in range(2):\n        bodies[at].x += 1.0\n        sum += bodies[at].x\n    sum\n\n\
             def main() -> Result[(), AllocError | AppendError[Body] | IoError]:\n    mut bodies = clist[Body]()\n    for _ in range({rows}):\n        bodies.append(Body(1.0))?\n    print(poke(&mut bodies, {at}))?\n    Ok(())\n"
        );
        let dir = loops_dir(&source);
        assert!(dump(dir.path(), "ops", "poke").contains("GetView("));
        for release in [false, true] {
            let mut command = std::process::Command::new(support::plenty_bin());
            command.current_dir(dir.path()).env("PLENTY_CACHE", "off");
            if !release {
                command.arg("--fast-compile");
            }
            let output = command.arg("loops.plenty").output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if rows == 0 || traps {
                assert!(!output.status.success(), "{source}");
                assert!(
                    stderr.starts_with("error: loops.plenty:7:9: clist index out of bounds"),
                    "{stderr}"
                );
            } else {
                assert!(output.status.success(), "{stderr}");
                assert_eq!(String::from_utf8_lossy(&output.stdout), "5.0\n");
            }
        }
    }
}

/// A clist reached through a class field (issue #759) caches through a shared
/// owner and falls back when the loop can resize or replace it through the
/// owner: a `&mut self` method, `&mut h.rows`, a field store, or an indirect
/// call.
const OWNERS: &str = r#"class B:
    x: f64


class H:
    rows: clist[B]

    def shrink(self: &mut H) -> ():
        self.rows.remove(0)
        ()


def make(n: i64, base: f64) -> Result[clist[B], AllocError | AppendError[B]]:
    mut rows = clist[B]()
    for k in range(n):
        rows.append(B(base + f64(k)))?
    Ok(rows)


def shrink_ref(rows: &mut clist[B]) -> ():
    rows.remove(0)
    ()


def read_owner(h: &H) -> f64:
    mut s = 0.0
    for i in range(len(h.rows)):
        s += h.rows[i].x
    s


def via_method(h: &mut H) -> f64:
    mut s = 0.0
    for _ in range(3):
        s += h.rows[0].x
        h.shrink()
    s


def via_field_call(h: &mut H) -> f64:
    mut s = 0.0
    for _ in range(3):
        s += h.rows[0].x
        shrink_ref(&mut h.rows)
    s


def via_field_assign(h: &mut H) -> Result[f64, AllocError | AppendError[B]]:
    mut s = 0.0
    for _ in range(2):
        s += h.rows[0].x
        h.rows = make(2, 100.0)?
    Ok(s)


def via_callable(rows: &mut clist[B], f: Callable[[&mut clist[B]], ()]) -> f64:
    mut s = 0.0
    for _ in range(3):
        s += rows[0].x
        f(rows)
    s


def main() -> Result[(), AllocError | AppendError[B] | IoError]:
    print(read_owner(&H(make(4, 10.0)?)))?
    mut a = H(make(4, 10.0)?)
    print(via_method(&mut a))?
    mut b = H(make(4, 10.0)?)
    print(via_field_call(&mut b))?
    mut c = H(make(4, 10.0)?)
    print(via_field_assign(&mut c)?)?
    mut d = make(4, 10.0)?
    print(via_callable(&mut d, shrink_ref))?
    Ok(())
"#;

#[test]
fn owner_paths_fall_back_when_the_loop_can_resize_the_clist() {
    let dir = loops_dir(OWNERS);
    let read = dump(dir.path(), "ops", "read_owner");
    assert!(read.contains("GetView("), "{read}");
    assert!(!read.contains("Clist(Get("), "{read}");
    for function in [
        "via_method",
        "via_field_call",
        "via_field_assign",
        "via_callable",
    ] {
        let ops = dump(dir.path(), "ops", function);
        assert!(ops.contains("Clist(Get("), "{function}: {ops}");
        assert!(!ops.contains("View("), "{function}: {ops}");
    }
    for release in [false, true] {
        let mut command = std::process::Command::new(support::plenty_bin());
        command.current_dir(dir.path()).env("PLENTY_CACHE", "off");
        if !release {
            command.arg("--fast-compile");
        }
        let output = command.arg("loops.plenty").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "46.0\n33.0\n33.0\n110.0\n33.0\n"
        );
    }
}

#[rstest]
#[case(false)]
#[case(true)]
fn zipped_columns_share_a_cursor_without_changing_remaining_ranges(#[case] release: bool) {
    let source = r#"
class Row:
    flag: bool
    small: i8
    medium: i16
    wide: u64
    value: f32

def first(values: &mut Iter[i8]) -> Option[i8]:
    next(values)

def visit(value: i8) -> i8:
    print("left").unwrap()
    value

def effects() -> ():
    mut left = clist[Row]()
    mut empty = clist[Row]()
    left.append(Row(True, 1i8, 1i16, 1u64, 1.0f32)).unwrap()
    # The left callback must still run once when the right side is empty.
    for a, b in left.column(Row.small).map(visit).zip(empty.column(Row.small)):
        check(False, "right empty").unwrap()

def main() -> Result[(), AppendError[Row] | IoError | OverflowError[i64]]:
    for length in range(18):
        mut left = clist[Row]()
        mut right = clist[Row]()
        for i in range(length):
            left.append(Row(i % 2 == 0, i8(i), i16(i), u64(i), f32(i)))?
        for i in range(length // 2):
            right.append(Row(True, i8(i), i16(i), u64(i), f32(i)))?
        mut seen = 0
        for flag, value in left.column(Row.flag).zip(left.column(Row.value)):
            check(flag == (i64(value) % 2 == 0), "mixed scalar columns").unwrap()
            seen += 1
        check(seen == length, "all rows").unwrap()
        # Empty and unequal owners still stop at the shorter side.
        check(left.column(Row.wide).zip(right.column(Row.small)).count() == length // 2, "right shortest").unwrap()
        check(right.column(Row.wide).zip(left.column(Row.small)).count() == length // 2, "left shortest").unwrap()
        mut a = left.column(Row.small)
        mut b = left.column(Row.wide)
        drop(next(a))
        drop(next(b))
        drop(next(b))
        mut pairs = a.zip(b)
        # next, then a terminal, must preserve the fused cursor's progress.
        match next(pairs):
            case Some((small, wide)):
                check(small == 1i8 and wide == 2u64, "shifted pair values").unwrap()
            case Nothing:
                check(length <= 2, "empty remaining range").unwrap()
        expected = length - 3 if length > 3 else 0
        check(pairs.count() == expected, "resumed zip").unwrap()
        for medium, small in left.column(Row.medium).zip(left.column(Row.small)):
            check(medium == i16(small), "two-byte column stride").unwrap()
        mut lent = left.column(Row.small)
        drop(first(&mut lent))
        mut n = 1
        for small, wide in lent.zip(left.column(Row.wide)):
            check(i64(small) == n and u64(n - 1) == wide, "previously lent column").unwrap()
            n += 1
    effects()
    print("ok")?
    Ok(())
"#;
    let output = run_mode(source, release);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"left\nok\n");
}

#[test]
fn selected_field_zip_avoids_nested_pull_loops_and_pending_options() {
    let dir = loops_dir(include_str!("../experiments/kernels/clist-zip/scan.plenty"));
    for function in ["scan_loop", "scan_fold"] {
        let ops = dump(dir.path(), "ops", function);
        assert_eq!(
            ops.lines().filter(|line| line.trim() == "Loop").count(),
            1,
            "{ops}"
        );
        assert!(!ops.contains("Option["), "{ops}");
    }
}
