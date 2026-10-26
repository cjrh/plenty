//! Scalar list elements reached through any borrowed place, including a field
//! (issue #122), are read and written inline, with no runtime call except the
//! out-of-bounds trap (issue #509).
mod support;
use support::assert_stdout as expect;

/// Names of the symbols `function` calls in a compiled object.
fn calls(source: &str, function: &str) -> Vec<String> {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("field.o");
    plenty::compile_source_to_object(source, &path).unwrap();
    let bytes = std::fs::read(path).unwrap();
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let symbol = file
        .symbols()
        .find(|s| s.name().is_ok_and(|name| name.ends_with(function)))
        .unwrap();
    let section = file
        .section_by_index(symbol.section_index().unwrap())
        .unwrap();
    section
        .relocations()
        .filter(|(offset, _)| {
            *offset >= symbol.address() && *offset < symbol.address() + symbol.size()
        })
        .filter_map(|(_, relocation)| match relocation.target() {
            RelocationTarget::Symbol(index) => Some(
                file.symbol_by_index(index)
                    .unwrap()
                    .name()
                    .unwrap()
                    .to_owned(),
            ),
            _ => None,
        })
        .collect()
}

fn runtime_calls(source: &str, function: &str) -> Vec<String> {
    calls(source, function)
        .into_iter()
        .filter(|name| name.starts_with("plenty_") && !name.starts_with("plenty_trap_"))
        .collect()
}

const PLACES: &str = r#"
class Inner:
    xs: list[i64]
    flags: list[bool]

class Outer:
    inner: Inner
    weights: list[f32]

    def bump(self: &mut Outer, i: i64) -> i64:
        self.inner.xs[i] += 1
        self.inner.xs[i]

def read(o: &Outer, i: i64) -> i64:
    o.inner.xs[i]

def write(o: &mut Outer, i: i64, v: i64) -> ():
    o.inner.xs[i] = v
    o.inner.flags[i] = v > 0
    o.weights[-1] = 0.5f32

def augment(o: &mut Outer, i: i64) -> ():
    o.inner.xs[i] *= 3
    o.weights[i] += 1.25f32

def nested(grid: &mut list[list[u8]], i: i64, j: i64) -> ():
    grid[i][j] = grid[i][j] + 1u8

def main() -> Result[(), AllocError | IoError]:
    mut o = Outer(Inner([1, 2, 3]?, [False, False, False]?), [1.0f32, 2.0f32]?)
    write(&mut o, 1, 7)
    augment(&mut o, 1)
    print(read(&o, 1), read(&o, -3), o.bump(0), o.inner.flags[1], o.inner.flags[2])?
    print(o.weights[0], o.weights[1])?
    mut grid = [[1u8, 2u8]?, [3u8, 4u8]?]?
    nested(&mut grid, 1, -1)
    print(grid[1][1])?
    Ok(())
"#;

#[test]
fn scalar_field_elements_need_no_runtime_calls() {
    for function in ["read", "write", "augment", "Outer.bump"] {
        assert_eq!(
            runtime_calls(PLACES, function),
            [] as [&str; 0],
            "{function}"
        );
        assert!(
            calls(PLACES, function)
                .iter()
                .any(|name| name == "plenty_trap_list_index"),
            "{function} keeps its bounds check"
        );
    }
    // The outer row is a list, found by `plenty_list_row`; the inner scalar
    // row is inline.
    assert_eq!(runtime_calls(PLACES, "nested"), ["plenty_list_row"; 2]);
    expect(PLACES, "21 1 2 True False\n1.0 1.75\n5\n");
}

/// The access forms `std.diff` uses: borrowed parameters indexed in `while`
/// loops, and list fields of a mutably borrowed record read and written in
/// the same loop.
const LOOPS: &str = r#"
class Frontier:
    forward: list[i64]
    offset: i64

def prefix(old: &list[i64], new: &list[i64]) -> i64:
    mut count = 0
    while count < len(old) and count < len(new) and old[count] == new[count]:
        count += 1
    count

def sweep(v: &mut Frontier, steps: i64) -> i64:
    at = v.offset
    mut k = steps
    while k >= -steps:
        if v.forward[at + k - 1] < v.forward[at + k + 1]:
            v.forward[at + k] = v.forward[at + k + 1]
        else:
            v.forward[at + k] = v.forward[at + k - 1] + 1
        k -= 2
    v.forward[at]

def main() -> Result[(), AllocError | IoError]:
    old = [1, 2, 3, 4]?
    new = [1, 2, 5]?
    mut v = Frontier([0, 3, 1, 4, 1, 5, 9]?, 3)
    print(prefix(&old, &new), sweep(&mut v, 2), v.forward)?
    Ok(())
"#;

#[test]
fn loop_indexing_of_borrowed_lists_is_inline() {
    for function in ["prefix", "sweep"] {
        assert_eq!(
            runtime_calls(LOOPS, function),
            [] as [&str; 0],
            "{function}"
        );
    }
    expect(LOOPS, "2 2 [0, 1, 1, 2, 1, 9, 9]\n");
}

#[test]
fn inline_rows_follow_pops_growth_and_negative_indices() {
    let source = r#"
def at(xs: &list[i64], i: i64) -> i64:
    xs[i]

def put(xs: &mut list[i64], i: i64, v: i64) -> ():
    xs[i] = v

def main() -> Result[(), AllocError | IoError]:
    mut xs = [10, 20, 30]?
    print(at(&xs, -1), at(&xs, -3))?
    put(&mut xs, -2, 21)
    for i in range(100):
        xs.append(i)?
    print(at(&xs, 1), at(&xs, 102), len(&xs))?
    xs.pop()
    print(at(&xs, -1), len(&xs))?
    at(&xs, 102)
    Ok(())
"#;
    let result = support::run(source);
    assert!(!result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "30 10\n21 99 103\n98 102\n"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("index out of bounds: index 102, length 102"),
        "{stderr}"
    );
}

#[test]
fn inline_writes_report_negative_indices_as_written() {
    let source = r#"
def put(xs: &mut list[bool], i: i64) -> ():
    xs[i] = True

def main() -> Result[(), AllocError | IoError]:
    mut xs = [False, False]?
    put(&mut xs, -2)
    print(xs)?
    put(&mut xs, -3)
    Ok(())
"#;
    let result = support::run(source);
    assert!(!result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "[True, False]\n");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("index out of bounds: index -3, length 2"),
        "{stderr}"
    );
}

#[test]
fn scalar_writes_keep_every_element_type_bits() {
    for (ty, value) in [
        ("i8", "-128i8"),
        ("i16", "-32768i16"),
        ("i32", "-2147483648i32"),
        ("i64", "-9223372036854775808"),
        ("u8", "255u8"),
        ("u16", "65535u16"),
        ("u32", "4294967295u32"),
        ("u64", "18446744073709551615u64"),
        ("f32", "-1.25f32"),
        ("f64", "-1.25"),
        ("bool", "True"),
    ] {
        let zero = if ty == "bool" {
            "False".to_owned()
        } else {
            format!("{ty}(0)")
        };
        let source = format!(
            "class Cell:\n    xs: list[{ty}]\ndef put(c: &mut Cell, v: {ty}) -> ():\n    c.xs[1] = v\ndef main() -> Result[(), AllocError | IoError]:\n    mut c = Cell(list[{ty}].new()?)\n    c.xs.append({zero})?\n    c.xs.append({zero})?\n    put(&mut c, {value})\n    mut xs = list[{ty}].new()?\n    xs.append(c.xs[0])?\n    xs[0] = {value}\n    print(xs[0], c.xs[1], xs[0] == c.xs[1])?\n    Ok(())\n"
        );
        let shown = if ty == "bool" {
            "True".to_owned()
        } else {
            value.trim_end_matches(ty).to_owned()
        };
        expect(&source, &format!("{shown} {shown} True\n"));
    }
}

#[test]
fn field_writes_keep_bounds_traps_and_evaluation_order() {
    let source = r#"
class Holder:
    xs: list[i64]

def index(n: i64) -> i64:
    print("index").unwrap()
    n

def value() -> i64:
    print("value").unwrap()
    9

def main() -> Result[(), AllocError | IoError]:
    mut h = Holder([1, 2]?)
    h.xs[index(-1)] = value()
    h.xs[index(0)] += value()
    print(h.xs[0], h.xs[1])?
    h.xs[index(2)] = value()
    Ok(())
"#;
    let result = support::run(source);
    assert!(!result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "value\nindex\nindex\nvalue\n10 9\nvalue\nindex\n"
    );
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("index out of bounds: index 2, length 2"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
