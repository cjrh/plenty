//! Dictionaries have no indexed assignment; `insert` adds or replaces,
//! `get_mut` lends a value in place (issue #580), `get_or_insert` lends
//! one after inserting a default for a missing key (issue #748), and
//! `(&mut d).values()` / `(&mut d).items()` lend every value (issue #803).
mod support;
use rstest::rstest;
use support::{assert_stdout, check_diagnostic as diagnostic};

const ASSIGNMENT: &str =
    "dictionaries have no indexed assignment; use `d.insert(k, v)?`, which adds or replaces";
const BORROW: &str = "dictionary values cannot be borrowed mutably by index; use `d.get_mut(k)`, which returns `Option[&mut V]`, or loop over `(&mut d).values()` or `(&mut d).items()` to change every value";

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

// Issue #931: the update passes its operands in registers; the dispatcher's
// argument block and descriptor checks are not on the path. A str key with
// a copyable value takes the helper whose inline-key hit is a leaf.
#[rstest]
#[case::text("str", "\"a\"", "plenty_dict_get_or_insert_text")]
#[case::integer("i64", "7", "plenty_dict_get_or_insert")]
fn get_or_insert_calls_the_narrow_helper(
    #[case] key: &str,
    #[case] literal: &str,
    #[case] helper: &str,
) {
    let source = format!(
        "def bump(counts: &mut dict[{key}, i64], key: {key}) -> Result[(), AllocError | OverflowError[i64]]:\n    *counts.get_or_insert(key, 0)? += 1\n    Ok(())\ndef main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n    mut counts = {{{literal}: 1}}?\n    bump(&mut counts, {literal})?\n    print(counts)?\n    Ok(())\n"
    );
    let calls = crate::test_text_bytes_fast_paths::runtime_calls(&source, "bump");
    assert!(calls.iter().any(|name| name == helper), "{calls:?}");
    assert!(
        !calls.iter().any(|name| name.starts_with("plenty_op_")),
        "{calls:?}"
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

const RESOURCE: &str = "class Resource:\n    name: str\n    hp: i64\n    log: list[str]\n    def __del__(self) -> ():\n        print(\"drop\", self.name).unwrap()\n";

#[test]
fn mutable_value_loops_change_affine_values_in_place_and_exit_early() {
    let source = format!(
        "{RESOURCE}def find(d: &mut dict[i64, Resource], want: i64) -> Result[bool, AllocError]:\n    for key, r in (&mut d).items():\n        r.log.append(\"seen\")?\n        if *key == want:\n            return Ok(True)\n    Ok(False)\ndef main() -> Result[(), AllocError | IoError | OverflowError[i64]]:\n    mut d = {{1: Resource(\"one\", 1, []?), 2: Resource(\"two\", 2, []?), 3: Resource(\"three\", 3, []?)}}?\n    for r in (&mut d).values():\n        r.hp += 10\n    print(find(&mut d, 2)?)?\n    for r in (&mut d).values():\n        if r.name == \"two\":\n            break\n        r.name = \"first\"\n    for key, r in d.items():\n        print(*key, r.name, r.hp, r.log)?\n    Ok(())\n"
    );
    assert_stdout(
        &source,
        "True\n1 first 11 [\"seen\"]\n2 two 12 [\"seen\"]\n3 three 13 []\ndrop first\ndrop two\ndrop three\n",
    );
}

#[test]
fn mutable_value_loops_reject_structural_mutation_and_other_access() {
    for body in [
        "    for v in (&mut lists).values():\n        lists.insert(\"b\", []?)?\n",
        "    for k, v in (&mut lists).items():\n        lists.pop(k)\n",
        "    for v in (&mut lists).values():\n        lists.clear()\n",
        "    for v in (&mut lists).values():\n        print(len(lists))?\n",
        "    for v in (&mut lists).values():\n        match lists.get_mut(\"a\"):\n            case Some(w):\n                w.append(1)?\n            case Nothing:\n                pass\n",
        "    for v in (&mut lists).values():\n        for w in (&mut lists).values():\n            w.append(1)?\n",
    ] {
        let error = diagnostic(&program(body));
        assert!(error.contains("conflicting borrow"), "{body}: {error}");
    }
}

#[test]
fn mutable_value_references_cannot_escape_the_loop() {
    for (body, expected) in [
        (
            "    mut refs: list[&mut list[i64]] = []?\n    for v in (&mut lists).values():\n        refs.append(v)?\n",
            "references cannot be stored in collections",
        ),
        (
            "    mut other = [0]?\n    mut keep = &mut other\n    for v in (&mut lists).values():\n        keep = v\n    keep.append(9)?\n",
            "can be reassigned only to a reference into the owner it first borrowed",
        ),
        (
            "    view = (&mut lists).values()\n",
            "mutable dictionary projections are immediate iteration sources",
        ),
    ] {
        let error = diagnostic(&program(body));
        assert!(error.contains(expected), "{body}: {error}");
    }
    let escape = "def first(fallback: &mut list[i64]) -> &mut list[i64]:\n    mut d = {\"a\": [1].unwrap()}.unwrap()\n    for v in (&mut d).values():\n        return v\n    fallback\ndef main() -> ():\n    pass\n";
    let error = diagnostic(escape);
    assert!(
        error.contains("returned reference must originate from the first reference parameter"),
        "{error}"
    );
}

#[test]
fn rust_style_mutable_iterators_name_the_exclusive_projection() {
    for (method, projection) in [
        ("values_mut", "values"),
        ("items_mut", "items"),
        ("iter_mut", "values"),
    ] {
        let body = format!("    for v in lists.{method}():\n        pass\n");
        let error = diagnostic(&program(&body));
        assert!(
            error.contains(&format!(
                "dictionaries have no `{method}`; borrow the dictionary exclusively instead: `for ... in (&mut d).{projection}():` yields `&mut V`"
            )),
            "{method}: {error}"
        );
    }
}

#[test]
fn writes_through_shared_value_loops_name_the_exclusive_projection() {
    const HINT: &str = "cannot borrow shared reference as mutable; `values()` and `items()` lend shared values; loop over `(&mut d).values()` or `(&mut d).items()` to change them";
    for body in [
        "    for v in lists.values():\n        v.append(2)?\n",
        "    for k, v in lists.items():\n        v.append(2)?\n",
        "    for p in points.values():\n        p.x = 2\n",
    ] {
        let error = diagnostic(&program(body));
        assert!(error.contains(HINT), "{body}: {error}");
    }
}
