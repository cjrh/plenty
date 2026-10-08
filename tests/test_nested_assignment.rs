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
