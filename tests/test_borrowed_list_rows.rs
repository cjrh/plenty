//! Borrowed list iteration (issue #368): each element reference is its row's
//! address from a cursor read once before the loop, not a generic element
//! lookup per iteration. References keep their semantics: loans, mutation
//! through `&mut`, returning an element, and nested and non-scalar rows.
mod support;

#[test]
fn returned_list_references_are_direct_iteration_sources() {
    support::assert_visible_stdout(
        r#"
class Columns:
    values: list[i64]
    def column(self) -> &list[i64]:
        &self.values
def column(values: &list[i64]) -> &list[i64]:
    values
def writable(values: &mut list[i64]) -> &mut list[i64]:
    values
def make() -> list[i64]:
    [8, 9].unwrap()
mut values = [1, 2, 3].unwrap()
print(iter(column(&values)).count()).unwrap()
for value in writable(&mut values):
    *value *= 2
for value in reversed(column(&values)):
    print(*value).unwrap()
items = Columns([4, 5].unwrap())
print(iter(items.column()).fold(0, def(a: i64, b: &i64) -> i64: a + *b)).unwrap()
for a, b in zip(column(&values), items.column()):
    print(*a + *b).unwrap()
for value in column(&make()):
    print(*value).unwrap()
"#,
        "3\n6\n4\n2\n9\n6\n9\n8\n9",
    );
}

#[test]
fn returned_iterator_sources_hold_the_owner_loan() {
    for action in ["values.append(4).unwrap()", "drop(values)"] {
        let source = format!(
            r#"
def column(values: &list[i64]) -> &list[i64]:
    values
mut values = [1, 2, 3].unwrap()
pipeline = iter(column(&values))
{action}
print(pipeline.count()).unwrap()
"#
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(error.contains("borrow"), "{error}");
    }
    support::assert_visible_stdout(
        r#"
def column(values: &list[i64]) -> &list[i64]:
    values
def make() -> list[i64]:
    [1].unwrap()
pipeline = iter(column(&make()))
print(pipeline.count()).unwrap()
"#,
        "1",
    );
}

use std::path::Path;
use std::process::Command;

const PROGRAM: &str = r#"class Point:
    x: i64
    y: i64

def sum_i8(values: &list[i8]) -> i64:
    mut total = 0
    for value in values:
        total += i64(*value)
    total

def sum_u8(values: &list[u8]) -> i64:
    mut total = 0
    for value in values:
        total += i64(*value)
    total

def sum_i16(values: &list[i16]) -> i64:
    mut total = 0
    for value in reversed(values):
        total += i64(*value)
    total

def sum_u16(values: &list[u16]) -> i64:
    mut total = 0
    for value in values:
        total += i64(*value)
    total

def sum_i32(values: &list[i32]) -> i64:
    mut total = 0
    for value in values:
        total += i64(*value)
    total

def sum_u32(values: &list[u32]) -> i64:
    mut total = 0
    for value in values:
        total += i64(*value)
    total

def sum_u64(values: &list[u64]) -> u64:
    mut total = 0u64
    for value in values:
        total = total.wrapping_add(*value)
    total

def sum_f32(values: &list[f32]) -> f64:
    mut total = 0.0
    for value in values:
        total += f64(*value)
    total

def sum_f64(values: &list[f64]) -> f64:
    mut total = 0.0
    for value in reversed(values):
        total += *value
    total

def count_true(flags: &list[bool]) -> i64:
    mut count = 0
    for flag in flags:
        if *flag:
            count += 1
    count

def find(values: &list[i64], wanted: i64) -> &i64:
    for value in values:
        if *value == wanted:
            return value
    &values[0]

def read(value: &i64) -> i64:
    *value

def first_negative(values: &list[i64]) -> i64:
    for value in values:
        if *value < 0:
            return read(value)
    0

class Bag:
    xs: list[i64]
    seen: i64

    def tally(self: &mut Bag) -> i64:
        mut total = 0
        for x in &self.xs:
            self.seen += 1
            total += *x
        total

def through(xs: &mut list[i64]) -> i64:
    mut total = 0
    for x in xs:
        *x = *x + 1
    for x in &*xs:
        total += *x
    total

def generic[T](xs: &list[T]) -> i64:
    mut n = 0
    for x in xs:
        n += 1
    n

def main() -> Result[(), AllocError | IoError]:
    print(sum_i8(&[-128i8, 127i8, -1i8, 5i8]?))?
    print(sum_u8(&[255u8, 1u8, 0u8]?))?
    print(sum_i16(&[-32768i16, 32767i16, -2i16]?))?
    print(sum_u16(&[65535u16, 1u16]?))?
    print(sum_i32(&[-2147483648i32, 2147483647i32, -3i32]?))?
    print(sum_u32(&[4294967295u32, 1u32]?))?
    empty: list[u64] = []?
    print(sum_u64(&empty), sum_u64(&[u64.MAX, 1u64, 9u64]?))?
    print(sum_f32(&[1.5f32, -0.25f32]?), sum_f64(&[0.5, 0.25, -2.0]?))?
    print(count_true(&[True, False, True, True]?))?

    mut numbers = [3, -7, 9, 1]?
    print(*find(&numbers, 9), *find(&numbers, 2), first_negative(&numbers))?
    for number in &numbers:
        if *number == 9:
            break
    numbers.append(4)?

    # Mutable iteration writes rows in place, forwards and reversed.
    mut bytes = [1u8, 2u8, 250u8]?
    for b in &mut bytes:
        *b = b.wrapping_add(10u8)
    print(bytes)?
    mut halves = [1.0f32, 3.0f32]?
    for h in reversed(&mut halves):
        *h = *h / 2.0f32
    print(halves)?
    mut toggles = [True, False]?
    for t in &mut toggles:
        *t = not *t
    print(toggles)?
    mut counter = 0
    for n in &mut numbers:
        counter += 1
        if counter == 2:
            continue
        if counter == 4:
            break
        *n = *n * 10
    print(numbers)?

    # Nested borrowed loops, empty and non-scalar rows.
    grid = [[1, 2]?, []?, [3]?]?
    mut cells = 0
    for row in &grid:
        for cell in row:
            cells += *cell
    print(cells)?
    words = ["ab", "cde"]?
    mut letters = 0
    for word in &words:
        letters += len(*word)
    print(letters)?
    mut points = [Point(1, 2), Point(3, 4)]?
    for point in &mut points:
        point.x = point.x + point.y
    for point in reversed(&points):
        print(point.x)?
    maybe = [Some(1), Nothing, Some(5)]?
    mut present = 0
    for item in &maybe:
        match item:
            case Some(v):
                present += *v
            case Nothing:
                pass
    print(present)?

    # The owned loop still copies scalars.
    mut owned_total = 0
    for value in [5, 6]?:
        owned_total += value
    print(owned_total)?

    mut bag = Bag([1, 2, 3]?, 0)
    print(bag.tally(), bag.seen)?
    mut ys = [10, 20]?
    print(through(&mut ys), ys)?
    print(generic(&ys), generic(&["a", "b", "c"]?))?
    print([*x * 2 for x in &ys]?)?

    # Growing an inner list does not move the outer list's rows.
    mut rows = [[1]?, [2, 3]?]?
    for row in &mut rows:
        for i in range(100):
            row.append(i)?
    mut all = 0
    for row in &rows:
        for v in row:
            all += *v
    print(all, len(rows[0]), len(rows[1]))?
    Ok(())
"#;

const EXPECTED: &str = "\
3
256
-3
65536
-4
4294967296
0 9
1.25 -1.25
3
9 3 -7
[11, 12, 4]
[0.5, 1.5]
[False, True]
[30, -7, 90, 1, 4]
6
5
7
3
6
11
6 3
32 [11, 21]
2 3
[22, 42]
9906 101 102
";

fn plenty(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "plenty {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("rows.plenty"), PROGRAM).unwrap();
    dir
}

#[test]
fn borrowed_list_iteration_keeps_reference_semantics() {
    let dir = setup();
    assert_eq!(plenty(dir.path(), &["rows.plenty"]), EXPECTED);
    assert_eq!(plenty(dir.path(), &["--release", "rows.plenty"]), EXPECTED);
}

#[test]
fn borrowed_list_iteration_still_holds_the_list_loan() {
    let shared = support::check_error(
        "mut xs = [1, 2].unwrap()\nfor x in &xs:\n    xs.append(*x).unwrap()\n",
    );
    assert!(shared.contains("while it is borrowed"), "{shared}");
    let exclusive = support::check_error(
        "mut xs = [1, 2].unwrap()\nfor x in &mut xs:\n    print(len(xs)).unwrap()\n",
    );
    assert!(
        exclusive.contains("while it is exclusively borrowed"),
        "{exclusive}"
    );
}

/// The hot loop of a borrowed scalar reduction makes no runtime call: the
/// length and row cursor are read once, and each element is a direct load.
#[test]
fn borrowed_scalar_iteration_loads_rows_directly() {
    let dir = setup();
    let ops = plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "ops",
            "--dump-filter",
            "sum_u64",
            "rows.plenty",
            "-o",
            "-",
        ],
    );
    assert!(ops.contains("ListRowRef(List(U64), false)"), "{ops}");
    assert!(!ops.contains("ElementRef"), "{ops}");
    assert!(!ops.contains("ReadRef(List"), "{ops}");
    let asm = plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "asm",
            "--dump-filter",
            "sum_u64",
            "rows.plenty",
            "-o",
            "-",
        ],
    );
    let calls: Vec<&str> = asm.lines().filter(|line| line.contains("call ")).collect();
    assert_eq!(calls.len(), 2, "{asm}");
    for helper in ["plenty_list_rows", "plenty_list_row_bytes"] {
        assert!(calls.iter().any(|call| call.ends_with(helper)), "{asm}");
    }
    for absent in [
        "plenty_op_",
        "plenty_retain",
        "plenty_release",
        "plenty_list_len",
    ] {
        assert!(!asm.contains(absent), "{asm}");
    }
}

/// A borrowed temporary list outlives the loop that reads its rows.
#[test]
fn borrowed_temporary_lists_live_through_the_loop() {
    support::assert_stdout(
        r#"
def make(n: i64) -> Result[list[i64], AllocError]:
    mut xs: list[i64] = []?
    for i in range(n):
        xs.append(i * i)?
    Ok(xs)

def words() -> Result[list[str], AllocError]:
    ["x", "yy", "zzz"]

def main() -> Result[(), AllocError | IoError]:
    mut total = 0
    for x in &make(5)?:
        total += *x
    for w in reversed(&words()?):
        total += len(*w)
    print(total)?
    Ok(())
"#,
        "36\n",
    );
}
