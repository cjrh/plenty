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
mut rows = [Row([1, 2].unwrap())].unwrap()
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

#[cfg(feature = "runtime-checks")]
#[test]
fn existing_nested_slots_need_no_allocation_including_inline_payloads() {
    expect(r#"
class Row:
    values: list[Option[range[i64]]]
mut rows = [Row([Some(range(3))].unwrap())].unwrap()
mut table = {"row": [1, 2].unwrap()}.unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
rows[0].values[0] = Some(range(5, 8))
table["row"][-1] = 9
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(rows[0].values[0]).unwrap()
print(table).unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\nOption[range].Some(range(5, 8, 1))\n{\"row\": [1, 9]}\n");
}

#[test]
fn replaced_owners_drop_once_and_failed_indices_clean_pending_values() {
    expect(
        r#"
class Item:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def missing() -> Result[i64, str]:
    Err("index failed")
def update(rows: &mut list[list[Item]]) -> Result[(), str]:
    rows[0][missing()?] = Item("pending")
    Ok(())
mut rows = [[Item("old")].unwrap()].unwrap()
rows[0][0] = Item("replacement")
print(update(&mut rows)).unwrap()
print(rows[0][0].label).unwrap()
drop(rows)
"#,
        "old\npending\nResult[(), str].Err(\"index failed\")\nreplacement\nreplacement\n",
    );
}

#[test]
fn outstanding_element_loans_and_moved_roots_still_reject_writes() {
    for source in [
        "mut rows = [[1].unwrap()].unwrap()\nr = &rows[0][0]\nrows[0][0] = 2\nprint(r).unwrap()",
        "mut rows = [[1].unwrap()].unwrap()\nr = &mut rows[0][0]\nrows[0][0] = 2\nprint(r).unwrap()",
        "rows = [[1].unwrap()].unwrap()\nrows[0][0] = 2",
        "def take(rows: list[list[i64]]) -> i64:\n    0\nmut rows = [[1].unwrap()].unwrap()\nrows[take(rows)][0] = 2",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[test]
fn invalid_nested_destinations_trap_instead_of_inserting() {
    for (source, diagnostic) in [
        (
            "mut rows = [[1].unwrap()].unwrap()\nrows[0][2] = 3",
            "index out of bounds",
        ),
        (
            "mut rows = [[1].unwrap()].unwrap()\nrows[-2][0] = 3",
            "index out of bounds",
        ),
        (
            "mut rows = {\"a\": [1].unwrap()}.unwrap()\nrows[\"b\"][0] = 3",
            "dictionary key not found",
        ),
        (
            "mut rows = [{\"a\": 1}.unwrap()].unwrap()\nrows[0][\"b\"] = 3",
            "dictionary key not found",
        ),
    ] {
        let output = support::run(source);
        assert!(!output.status.success(), "{source}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
