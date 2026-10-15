//! `for` over a literal `range(...)` is a counted loop: its count is computed
//! once and the loop never calls the runtime (issue #121).
mod support;

fn expect(source: &str, output: &str) {
    let result = support::run(source);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), output);
}

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
def main() -> Result[(), Failure]:
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
def main() -> Result[(), Failure]:
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
def main() -> Result[(), Failure]:
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
