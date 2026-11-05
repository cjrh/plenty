//! Dictionaries have no indexed assignment; `insert` adds or replaces,
//! `get_mut` lends a value in place (issue #580), and `get_or_insert` lends
//! one after inserting a default for a missing key (issue #748).
mod support;
use support::{assert_stdout, check_diagnostic as diagnostic};

const ASSIGNMENT: &str =
    "dictionaries have no indexed assignment; use `d.insert(k, v)?`, which adds or replaces";
const BORROW: &str = "dictionary values cannot be borrowed mutably by index; use `d.get_mut(k)`, which returns `Option[&mut V]`";

fn program(body: &str) -> String {
    format!(
        "class Point:\n    x: i64\ndef main() -> Result[(), AllocError | IoError]:\n    mut d = {{\"a\": 1}}?\n    mut lists = {{\"a\": [1]?}}?\n    mut points = {{\"a\": Point(1)}}?\n{body}    Ok(())\n"
    )
}

#[test]
fn indexed_assignment_names_insert() {
    for body in [
        "    d[\"b\"] = 2\n",
        "    d[\"a\"] = 2\n",
        "    d[\"a\"], d[\"b\"] = 2, 3\n",
    ] {
        assert_eq!(
            diagnostic(&program(body)),
            format!("7:5: {ASSIGNMENT}"),
            "{body}"
        );
    }
}

#[test]
fn augmented_assignment_names_insert_and_get_mut() {
    for op in ["+", "-", "*"] {
        let body = format!("    d[\"a\"] {op}= 2\n");
        assert_eq!(
            diagnostic(&program(&body)),
            format!(
                "7:5: dictionaries have no indexed assignment; use `d.insert(k, d.get(k, 0) {op} v)?`, or update the value in place through `d.get_mut(k)`, which returns `Option[&mut V]`"
            ),
            "{body}"
        );
    }
}

#[test]
fn mutable_index_borrows_name_get_mut() {
    for (body, column) in [
        ("    r = &mut d[\"a\"]\n    *r = 2\n", 14),
        ("    lists[\"a\"].append(2)?\n", 5),
        ("    points[\"a\"].x = 2\n", 5),
        ("    old = replace(d[\"a\"], 2)\n", 19),
    ] {
        assert_eq!(
            diagnostic(&program(body)),
            format!("7:{column}: {BORROW}"),
            "{body}"
        );
    }
}

#[test]
fn reads_by_index_still_compile() {
    assert_stdout(
        &program("    r = &d[\"a\"]\n    print(d[\"a\"], *r, points[\"a\"].x, lists[\"a\"][0])?\n"),
        "1 1 1 1\n",
    );
}

#[test]
fn get_mut_changes_present_values_in_place() {
    assert_stdout(
        &program(
            "    match d.get_mut(\"a\"):\n        case Some(count):\n            *count += 10\n        case Nothing:\n            pass\n    match lists.get_mut(\"a\"):\n        case Some(list):\n            list.append(2)?\n        case Nothing:\n            pass\n    match points.get_mut(\"a\"):\n        case Some(point):\n            point.x = 5\n        case Nothing:\n            pass\n    print(d, lists, points[\"a\"].x)?\n",
        ).replace("AllocError | IoError", "AllocError | IoError | OverflowError[i64]"),
        "{\"a\": 11} {\"a\": [1, 2]} 5\n",
    );
}

#[test]
fn get_mut_returns_nothing_for_an_absent_key() {
    assert_stdout(
        &program(
            "    match d.get_mut(\"missing\"):\n        case Some(_):\n            print(\"found\")?\n        case Nothing:\n            print(\"absent\")?\n    print(d)?\n",
        ),
        "absent\n{\"a\": 1}\n",
    );
}

#[test]
fn get_mut_works_through_a_mutable_reference() {
    assert_stdout(
        "def bump(counts: &mut dict[str, i64], key: str) -> Result[(), OverflowError[i64]]:\n    match counts.get_mut(key):\n        case Some(count):\n            *count += 1\n        case Nothing:\n            pass\n    Ok(())\ndef main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n    mut counts = {\"a\": 1}?\n    bump(&mut counts, \"a\")?\n    bump(&mut counts, \"b\")?\n    print(counts)?\n    Ok(())\n",
        "{\"a\": 2}\n",
    );
}

#[test]
fn get_mut_borrows_the_whole_dictionary() {
    for body in [
        "    match d.get_mut(\"a\"):\n        case Some(count):\n            d.insert(\"b\", 2)?\n            *count = 3\n        case Nothing:\n            pass\n",
        "    match d.get_mut(\"a\"):\n        case Some(count):\n            print(d[\"a\"])?\n            *count = 3\n        case Nothing:\n            pass\n",
    ] {
        let error = diagnostic(&program(body));
        assert!(error.contains("conflicting borrow"), "{body}: {error}");
    }
}

#[test]
fn get_mut_requires_a_mutable_binding_and_one_key() {
    let immutable = "def main() -> Result[(), AllocError | IoError]:\n    d = {\"a\": 1}?\n    match d.get_mut(\"a\"):\n        case Some(count):\n            *count = 2\n        case Nothing:\n            pass\n    Ok(())\n";
    assert!(
        diagnostic(immutable).contains("immutable binding"),
        "{}",
        diagnostic(immutable)
    );
    let error = diagnostic(&program("    value = d.get_mut(\"a\", \"b\")\n"));
    assert!(
        error.contains("get_mut requires one key argument"),
        "{error}"
    );
    let error = diagnostic(&program("    value = d.get_mut(1)\n"));
    assert!(error.contains("expected str, got i64"), "{error}");
}

#[test]
fn replace_through_get_mut_keeps_the_entry_position() {
    assert_stdout(
        "class Guard:\n    label: str\n    def __del__(self) -> ():\n        print(\"drop\", self.label).unwrap()\ndef main() -> Result[(), AllocError | IoError]:\n    mut guards = {\"a\": Guard(\"old\"), \"b\": Guard(\"b\")}?\n    match guards.get_mut(\"a\"):\n        case Some(guard):\n            old = replace(*guard, Guard(\"new\"))\n            print(\"took\", old.label)?\n        case Nothing:\n            pass\n    for key in guards.keys():\n        print(*key)?\n    Ok(())\n",
        "took old\ndrop old\na\nb\ndrop new\ndrop b\n",
    );
    let error = diagnostic(
        "def main() -> Result[(), AllocError | IoError]:\n    values = [1]?\n    view = &values\n    old = replace(*view, [2]?)\n    Ok(())\n",
    );
    assert!(
        error.contains("cannot borrow shared reference as mutable"),
        "{error}"
    );
}

#[test]
fn get_or_insert_counts_and_groups_with_one_lookup() {
    assert_stdout(
        "def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n    mut counts: dict[str, i64] = {}?\n    for word in [\"a\", \"b\", \"a\", \"c\", \"a\"]?:\n        *counts.get_or_insert(word, 0)? += 1\n    mut groups: dict[bool, list[i64]] = {}?\n    for n in range(5):\n        group = groups.get_or_insert(n > 2, []?)?\n        group.append(n)?\n    first = counts.get_or_insert(\"a\", 100)?\n    print(*first)?\n    print(counts, groups)?\n    Ok(())\n",
        "3\n{\"a\": 3, \"b\": 1, \"c\": 1} {False: [0, 1, 2], True: [3, 4]}\n",
    );
}

#[test]
fn get_or_insert_drops_an_unused_default_and_keeps_the_entry() {
    assert_stdout(
        "class Guard:\n    label: str\n    def __del__(self) -> ():\n        print(\"drop\", self.label).unwrap()\ndef main() -> Result[(), AllocError | IoError]:\n    mut guards = {\"a\": Guard(\"a\")}?\n    hit = guards.get_or_insert(\"a\", Guard(\"unused\"))?\n    print(\"hit\", hit.label)?\n    missed = guards.get_or_insert(\"b\", Guard(\"b\"))?\n    missed.label = \"b2\"\n    for key in guards.keys():\n        print(*key)?\n    Ok(())\n",
        "drop unused\nhit a\na\nb\ndrop a\ndrop b2\n",
    );
}

#[test]
fn get_or_insert_works_through_a_mutable_reference() {
    assert_stdout(
        "def bump(counts: &mut dict[str, i64], key: str) -> Result[(), AllocError | OverflowError[i64]]:\n    *counts.get_or_insert(key, 0)? += 1\n    Ok(())\ndef main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n    mut counts = {\"a\": 1}?\n    bump(&mut counts, \"a\")?\n    bump(&mut counts, \"b\")?\n    print(counts)?\n    Ok(())\n",
        "{\"a\": 2, \"b\": 1}\n",
    );
}

#[test]
fn get_or_insert_borrows_the_whole_dictionary() {
    let error = diagnostic(&program(
        "    count = d.get_or_insert(\"b\", 2)?\n    print(d)?\n    *count = 3\n",
    ));
    assert!(error.contains("conflicting borrow"), "{error}");
}

#[test]
fn get_or_insert_requires_a_mutable_binding_a_key_and_a_default() {
    let immutable = "def main() -> Result[(), AllocError | IoError]:\n    d = {\"a\": 1}?\n    *d.get_or_insert(\"a\", 0)? = 2\n    Ok(())\n";
    assert!(
        diagnostic(immutable).contains("immutable binding"),
        "{}",
        diagnostic(immutable)
    );
    let error = diagnostic(&program("    value = d.get_or_insert(\"a\")?\n"));
    assert!(
        error.contains("get_or_insert requires a key and a default value"),
        "{error}"
    );
    let error = diagnostic(&program("    value = d.get_or_insert(\"a\", \"b\")?\n"));
    assert!(error.contains("expected i64, got str"), "{error}");
    let error = diagnostic(&program("    value = d.get_or_insert(1, 2)?\n"));
    assert!(error.contains("expected str, got i64"), "{error}");
}
