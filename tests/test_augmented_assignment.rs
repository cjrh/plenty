//! Arithmetic updates of writable places with one evaluation of each index.
mod support;

fn expect(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn numeric_operators_keep_the_destination_type() {
    expect(
        r#"
mut count: i32 = 20
count+=2
count -= 3
count *= 2
count //= 3
count %= 5
mut fraction: f32 = 4
fraction /= 2
fraction += 0.5
mut unsigned: u8 = 3
unsigned += unsigned
print((count, fraction, unsigned)).unwrap()
"#,
        "(2, 2.5, 6)\n",
    );
}

#[test]
fn nested_target_indices_run_once_before_the_rhs() {
    expect(
        r#"
class Row:
    values: list[i64]
def mark(label: i64, index: i64) -> i64:
    print(label).unwrap()
    index
mut rows = [Row([2, 5].unwrap())].unwrap()
rows[mark(1, 0)].values[mark(2, -1)] += mark(3, 7)
print(rows).unwrap()
mut boxes = [Box(5).unwrap()].unwrap()
*boxes[mark(4, 0)] += mark(5, 7)
print(boxes).unwrap()
"#,
        "1\n2\n3\n[Row(values=[2, 12])]\n4\n5\n[Box(12)]\n",
    );
}

#[test]
fn saved_string_keys_and_references_work_in_generic_functions() {
    expect(
        r#"
def key() -> str:
    print("key").unwrap()
    "row"
def add[T](value: &mut T, amount: T) -> ():
    *value += amount
mut table = {"row": [4].unwrap()}.unwrap()
table[key()][-1] += len(table["row"])
add(&mut table["row"][0], 2)
mut score = 5
reference = &mut score
reference *= 3
print(table).unwrap()
print(score).unwrap()
"#,
        "key\n{\"row\": [7]}\n15\n",
    );
}

#[test]
fn index_expressions_can_mutate_before_the_target_is_read() {
    expect(
        r#"
def extend(rows: &mut list[list[i64]]) -> i64:
    rows[0].append(4).unwrap()
    -1
mut rows = [[1, 2].unwrap()].unwrap()
rows[0][extend(&mut rows)] += 3
print(rows).unwrap()
"#,
        "[[1, 2, 7]]\n",
    );
}

#[test]
fn rhs_failure_preserves_the_destination() {
    expect(
        r#"
def fail() -> Result[i64, str]:
    Err("failed")
def update(rows: &mut list[list[i64]]) -> Result[(), str]:
    rows[0][0] += fail()?
    Ok(())
def update_index(rows: &mut list[list[i64]]) -> Result[(), str]:
    rows[0][fail()?] += 2
    Ok(())
mut rows = [[9].unwrap()].unwrap()
print(str.repr(update(&mut rows)).unwrap()).unwrap()
print(str.repr(update_index(&mut rows)).unwrap()).unwrap()
print(rows).unwrap()
"#,
        "Result[(), str].Err(\"failed\")\nResult[(), str].Err(\"failed\")\n[[9]]\n",
    );
}

#[test]
fn rhs_cannot_replace_resize_or_move_the_destination_owner() {
    for source in [
        "def change(rows: &mut list[list[i64]]) -> i64:\n    rows.clear()\n    rows.append([8].unwrap()).unwrap()\n    2\nmut rows = [[1].unwrap()].unwrap()\nrows[0][0] += change(&mut rows)",
        "def take(rows: list[list[i64]]) -> i64:\n    2\nmut rows = [[1].unwrap()].unwrap()\nrows[0][0] += take(rows)",
        "mut rows = [[1].unwrap()].unwrap()\nr = &rows[0][0]\nrows[0][0] += 2\nprint(r).unwrap()",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[test]
fn invalid_targets_and_operator_types_are_rejected() {
    for (source, message) in [
        ("score = 2\nscore += 1", "immutable binding"),
        ("rows = [2].unwrap()\nrows[0] += 1", "immutable binding"),
        ("score += 1", "existing writable"),
        ("mut score: i32 = 2\nscore += 1i64", "expected i32"),
        ("mut score = 2\nscore /= 1", "use `//`"),
        ("mut score = 2.0\nscore //= 1.0", "require integers"),
        ("mut text = \"hi\"\ntext += \"!\"", "numeric destination"),
        ("mut flag = True\nflag += False", "numeric destination"),
        ("mut score = 2\nr = &score\n*r += 1", "shared reference"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(message), "{source}\n{error}");
    }
}

#[test]
fn closure_captures_and_generator_bodies_use_the_same_updates() {
    expect(
        r#"
def values() -> Generator[i64]:
    mut value = 1
    while value < 3:
        yield value
        value += 1
mut total = 0
mut add = def [&mut total](value: i64) -> ():
    total += value
for value in values():
    add(value)
print(total).unwrap()
"#,
        "3\n",
    );
}

#[test]
fn constructors_must_initialize_fields_before_updating_them() {
    expect(
        r#"
class Count:
    value: i64
    def __init__(self: &mut Count) -> ():
        self.value = 2
        self.value += self.value
print(Count()).unwrap()
"#,
        "Count(value=4)\n",
    );
    assert!(support::check_source("class Count:\n    value: i64\n    def __init__(self: &mut Count) -> ():\n        self.value += 1").is_err());
}

#[test]
fn missing_target_slots_trap_before_evaluating_the_rhs() {
    for source in [
        "def rhs() -> i64:\n    print(\"rhs\").unwrap()\n    2\nmut values = [1].unwrap()\nvalues[1] += rhs()",
        "def rhs() -> i64:\n    print(\"rhs\").unwrap()\n    2\nmut values = {\"found\": 1}.unwrap()\nvalues[\"missing\"] += rhs()",
    ] {
        let output = support::run(source);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty(), "{}", String::from_utf8_lossy(&output.stdout));
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn numeric_updates_allocate_nothing() {
    expect(r#"
key = str.repr(123456789).unwrap()
mut table = {key: [4].unwrap()}.unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
table[key][0] += 3
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(table).unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n{\"123456789\": [7]}\n");
}
