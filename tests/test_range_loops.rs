//! `for` over a literal `range(...)` is a counted loop: its count is computed
//! once and the loop never calls the runtime (issue #121). A stored range,
//! including a generator expression's outer source, runs the same loop from
//! its validated start, step, and count (issue #371).
mod support;
use support::assert_stdout as expect;

/// Names of the symbols `function` calls in a compiled object.
fn calls(source: &str, function: &str) -> Vec<String> {
    use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("loop.o");
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

#[test]
fn counted_loops_call_the_runtime_only_to_report_an_invalid_range() {
    let source = r#"
def total(n: i64, step: i64) -> i64:
    mut sum = 0
    for i in range(n):
        sum += i
    for i in reversed(range(n, 0, step)):
        sum += i
    for i, j in zip(range(3u8, 9u8), range(n)):
        sum += i64(i) * j
    sum
def main() -> ():
    print(total(10, -2)).unwrap()
"#;
    let calls = calls(source, "total");
    let runtime: Vec<_> = calls
        .iter()
        .filter(|name| name.starts_with("plenty_") && !name.starts_with("plenty_trap_"))
        .collect();
    // One cold call per range reports a zero step or an oversized length.
    assert_eq!(runtime, ["plenty_collection"; 4], "{calls:?}");
    expect(source, "175\n");
}

#[test]
fn counted_loops_keep_range_semantics() {
    expect(
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut a: list[i64] = []?
    for i in range(5):
        a.append(i)?
    for i in range(-3, 3):
        a.append(i)?
    for i in range(10, -10, -3):
        a.append(i)?
    for i in range(0, 10, 4):
        a.append(i)?
    for i in range(5, 5):
        a.append(i)?
    for i in range(5, 0):
        a.append(i)?
    print(a)?
    mut b: list[i64] = []?
    # The count is fixed when the loop starts.
    mut n = 4
    for i in range(n):
        n += 10
        b.append(i)?
    for i in reversed(range(1, 10, 3)):
        b.append(i)?
    for i in reversed(range(10, 1, -4)):
        b.append(i)?
    for i in range(10):
        if i % 2 == 0:
            continue
        if i > 7:
            break
        b.append(i)?
    for k, v in enumerate(range(100, 103)):
        b.append(k * 1000 + v)?
    for x, y in zip(range(3), range(10, 0, -1)):
        b.append(x * 100 + y)?
    print(b)?
    print([i * i for i in range(1, 8, 2)]?)?
    print(all(i < 5 for i in range(5)))?
    print(any(i == 4 for i in reversed(range(3))))?
    Ok(())
"#,
        "[0, 1, 2, 3, 4, -3, -2, -1, 0, 1, 2, 10, 7, 4, 1, -2, -5, -8, 0, 4, 8]\n\
         [0, 1, 2, 3, 7, 4, 1, 2, 6, 10, 1, 3, 5, 7, 100, 1101, 2102, 10, 109, 208]\n\
         [1, 9, 25, 49]\n\
         True\n\
         False\n",
    );
}

#[test]
fn counted_loops_reach_the_ends_of_every_integer_type() {
    expect(
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut c: list[i64] = []?
    for v in range(250u8, 255u8):
        c.append(i64(v))?
    for v in range(-128i8, 127i8, 50):
        c.append(i64(v))?
    for v in range(127i8, -128i8, -100):
        c.append(i64(v))?
    for v in reversed(range(0u8, 255u8, 60)):
        c.append(i64(v))?
    for v in range(18446744073709551610u64, 18446744073709551615u64, 2):
        c.append(i64.wrapping_from(v))?
    for v in range(9223372036854775800, 9223372036854775807, 3):
        c.append(v)?
    for v in range(-9223372036854775807 - 1, -9223372036854775800, 3):
        c.append(v)?
    for v in range(0, 9223372036854775807, 4611686018427387904):
        c.append(v)?
    for v in range(10, -10, -9223372036854775807 - 1):
        c.append(v)?
    for v in reversed(range(-9223372036854775807 - 1, 9223372036854775807, 9223372036854775807)):
        c.append(v)?
    print(c)?
    Ok(())
"#,
        "[250, 251, 252, 253, 254, -128, -78, -28, 22, 72, 122, 127, 27, -73, \
         240, 180, 120, 60, 0, -6, -4, -2, 9223372036854775800, 9223372036854775803, \
         9223372036854775806, -9223372036854775808, -9223372036854775805, \
         -9223372036854775802, 0, 4611686018427387904, 10, 9223372036854775806, -1, \
         -9223372036854775808]\n",
    );
}

#[test]
fn stored_range_loops_never_call_the_runtime() {
    let source = r#"
class Span:
    r: range
def total(r: range, s: range[u8], span: &Span) -> i64:
    mut sum = 0
    for i in r:
        sum += i
    for i in reversed(r):
        sum += i
    for i in &r:
        sum += *i
    for i, j in zip(s, r):
        sum += i64(i) * j
    for k, i in enumerate(span.r):
        sum += k * i
    sum
def main() -> ():
    print(total(range(10, 0, -2), range(3u8, 9u8), &Span(range(4)))).unwrap()
"#;
    // Copying a range for a loop or a parameter slot also needs no call.
    let calls = calls(source, "_total");
    assert!(
        !calls
            .iter()
            .any(|name| name.starts_with("plenty_") && !name.starts_with("plenty_trap_")),
        "{calls:?}"
    );
    expect(source, "234\n");
}

#[test]
fn generator_expression_range_sources_are_counted_loops() {
    let source = r#"
def main() -> Result[(), IoError]:
    mut total: i64 = 0
    for v in (n * 3 for n in range(10, -10, -3)):
        total += v
    print(total)?
    Ok(())
"#;
    let calls = calls(source, "__plenty_resume___plenty_genexpr_main_4_14");
    // The frame calls the runtime once, when it finishes.
    let runtime: Vec<_> = calls
        .iter()
        .filter(|name| name.starts_with("plenty_") && !name.starts_with("plenty_trap_"))
        .collect();
    assert_eq!(runtime, ["plenty_generator_finish"], "{calls:?}");
    expect(source, "21\n");
}

#[test]
fn stored_range_loops_keep_range_semantics() {
    expect(
        r#"
class Span:
    r: range
    def total(self: &Span) -> i64:
        mut t = 0
        for i in self.r:
            t += i
        t
def make(n: i64) -> range:
    range(n, 0, -2)
def main() -> Result[(), AllocError | IoError]:
    mut a: list[i64] = []?
    # The loop copies the range when it starts.
    mut r = range(10, -10, -3)
    for i in r:
        a.append(i)?
        r = range(1)
    for i in r:
        a.append(i)?
    e = range(5, 0)
    for i in e:
        a.append(i)?
    for i in reversed(e):
        a.append(i)?
    print(a)?
    mut b: list[i64] = []?
    s = range(1, 10, 3)
    for i in reversed(s):
        b.append(i)?
    for i in &s:
        b.append(*i)?
    for k, v in enumerate(s):
        b.append(k * 1000 + v)?
    for x, y in zip(s, range(10, 0, -1)):
        b.append(x * 100 + y)?
    for i in make(7):
        b.append(i)?
    b.append(Span(range(4)).total())?
    print(b)?
    print([i * i for i in s]?)?
    print(list(n * 2 for n in range(10, 0, -3))?)?
    print(list(n for n in reversed(range(3)))?)?
    print(list(n for n in range(5, 5))?)?
    print(any(i == 4 for i in range(250u8, 255u8)))?
    Ok(())
"#,
        "[10, 7, 4, 1, -2, -5, -8, 0]\n\
         [7, 4, 1, 1, 4, 7, 1, 1004, 2007, 110, 409, 708, 7, 5, 3, 1, 6]\n\
         [1, 16, 49]\n\
         [20, 14, 8, 2]\n\
         [2, 1, 0]\n\
         []\n\
         False\n",
    );
}

#[test]
fn stored_range_loops_reach_the_ends_of_every_integer_type() {
    expect(
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut c: list[i64] = []?
    for v in (x for x in range(250u8, 255u8)):
        c.append(i64(v))?
    narrow = range(127i8, -128i8, -100)
    for v in narrow:
        c.append(i64(v))?
    for v in reversed(range(0u8, 255u8, 60)):
        c.append(i64(v))?
    u = range(18446744073709551610u64, 18446744073709551615u64, 2)
    for v in u:
        c.append(i64.wrapping_from(v))?
    for v in (x for x in range(9223372036854775800, 9223372036854775807, 3)):
        c.append(v)?
    for v in (x for x in range(10, -10, -9223372036854775807 - 1)):
        c.append(v)?
    big = range(-9223372036854775807 - 1, 9223372036854775807, 9223372036854775807)
    for v in reversed(big):
        c.append(v)?
    print(c)?
    Ok(())
"#,
        "[250, 251, 252, 253, 254, 127, 27, -73, 240, 180, 120, 60, 0, -6, -4, -2, \
         9223372036854775800, 9223372036854775803, 9223372036854775806, 10, \
         9223372036854775806, -1, -9223372036854775808]\n",
    );
}

#[test]
fn unit_results_with_managed_errors_return_ok_and_err() {
    expect(
        r#"
enum Problem:
    Syntax(str)
    Limit(i64)
class Meter:
    work: i64
    def charge(self: &mut Meter) -> Result[(), Problem]:
        self.work += 1
        if self.work == 3:
            return Err(Problem.Syntax("a message longer than inline storage"))
        Ok(())
def run(m: &mut Meter) -> Result[i64, Problem]:
    for _ in range(5):
        m.charge()?
    Ok(m.work)
def main() -> Result[(), IoError]:
    mut m = Meter(0)
    match run(&mut m):
        case Ok(n):
            print(n)?
        case Err(Problem.Syntax(message)):
            print(message)?
        case Err(Problem.Limit(n)):
            print(n)?
    m.work = 10
    match run(&mut m):
        case Ok(n):
            print(n)?
        case Err(_):
            print("error")?
    Ok(())
"#,
        "a message longer than inline storage\n15\n",
    );
}
