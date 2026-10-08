//! Native writes through nested places, including evaluation and loan lifetimes.
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
fn nested_indices_run_once_after_the_value_in_path_order() {
    expect(
        r#"
def mark(label: i64, value: i64) -> i64:
    print(label).unwrap()
    value
mut rows = [[1, 2].unwrap()].unwrap()
rows[mark(1, 0)][mark(2, -1)] = mark(0, 9)
print(rows).unwrap()
"#,
        "0\n1\n2\n[[1, 9]]\n",
    );
}

#[test]
fn nested_class_fields_and_reference_parameters_share_write_semantics() {
    expect(
        r#"
class Row:
    values: list[i64]
def change(rows: &mut list[Row]) -> ():
    rows[0].values[-1] = 7
mut rows = [Row([1, 2].unwrap()).unwrap()].unwrap()
change(&mut rows)
rows[0].values[0] = 8
print(rows).unwrap()
"#,
        "[Row(values=[8, 7])]\n",
    );
}

#[test]
fn index_expressions_can_read_the_destination_before_its_exclusive_loan() {
    expect(
        r#"
mut rows = [[1, 2].unwrap()].unwrap()
rows[len(rows) - 1][len(rows[0]) - 1] = 9
print(rows).unwrap()
"#,
        "[[1, 9]]\n",
    );
}

#[test]
fn rhs_and_indices_can_resize_before_addresses_are_resolved() {
    expect(
        r#"
def replace(rows: &mut list[list[i64]]) -> i64:
    rows.clear()
    rows.append([3].unwrap()).unwrap()
    8
def extend(rows: &mut list[list[i64]]) -> i64:
    rows.reserve(100).unwrap()
    rows[0].append(4).unwrap()
    -1
mut rows = [[1, 2].unwrap()].unwrap()
rows[0][extend(&mut rows)] = replace(&mut rows)
print(rows).unwrap()
"#,
        "[[3, 8]]\n",
    );
}
