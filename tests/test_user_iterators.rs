mod support;
use std::process::Command;

fn run(source: &str, expected: &str) {
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("program");
        plenty::compile_source_to_executable_with_options(
            &support::program(source),
            &executable,
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("optimize={optimize}: {source}\n{error}"));
        let output = Command::new(executable).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "optimize={optimize}"
        );
    }
}

fn reject(source: &str, expected: &str) {
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let error = plenty::compile_source_to_executable_with_options(
            &support::program(source),
            &dir.path().join("program"),
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .err()
        .unwrap_or_else(|| panic!("optimize={optimize}: unexpectedly accepted {source}"))
        .to_string();
        assert!(
            error.contains(expected),
            "optimize={optimize}: {source}\nexpected {expected}, got {error}"
        );
    }
}

const COUNTER: &str = r#"
class Counter:
    position: i64
    stop: i64
    def next(self: &mut Counter) -> Option[i64]:
        if self.position >= self.stop:
            return Nothing
        value = self.position
        # Fixture cursor starts at zero and stops at a small positive bound.
        unchecked:
            self.position += 1
        Some(value)
"#;

const CURSOR: &str = r#"
class Cursor[T]:
    data: &[T]
    position: i64
    def next(self: &mut Cursor[T]) -> Option[&T]:
        if self.position >= len(self.data):
            return Nothing
        index = self.position
        unchecked:
            self.position += 1
        Some(&self.data[index])
"#;

#[test]
fn owned_user_iteration_and_lending_keep_progress() {
    run(
        &format!(
            "{COUNTER}\nfor value in Counter(0, 3):\n    print(value).unwrap()\nmut c = Counter(0, 4)\nprint(next(c)).unwrap()\nfor value in &mut c:\n    print(value).unwrap()\n    break\nprint(next(&mut c)).unwrap()\nfor value in &mut c:\n    print(value).unwrap()\nprint(next(c)).unwrap()"
        ),
        "0\n1\n2\nOption[i64].Some(0)\n1\nOption[i64].Some(2)\n3\nOption[i64].Nothing\n",
    );
}

#[test]
fn generic_view_iterator_has_strict_reference_items() {
    run(
        &format!(
            "{CURSOR}\nvalues = [2, 4, 6].unwrap()\nmut cursor = Cursor(&values, 0)\nfirst = next(&mut cursor).unwrap()\nprint(*first).unwrap()\nfor value in &mut cursor:\n    print(*value).unwrap()\nprint(len(values)).unwrap()"
        ),
        "2\n4\n6\n3\n",
    );
    reject(
        &format!(
            "{CURSOR}\nvalues = [2, 4].unwrap()\nmut cursor = Cursor(&values, 0)\nfirst = next(&mut cursor).unwrap()\nsecond = next(&mut cursor)\nprint(first, second).unwrap()"
        ),
        "borrow",
    );
}

#[test]
fn owned_user_sources_enter_existing_pipelines_and_comprehensions() {
    run(
        &format!(
            "{COUNTER}\nvalues = [n for n in Counter(0, 3)].unwrap()\nprint(values).unwrap()\nprint(iter(Counter(0, 4)).count()).unwrap()\nfor index, value in enumerate(Counter(0, 2)):\n    print(index, value).unwrap()"
        ),
        "[0, 1, 2]\n4\n0 0\n1 1\n",
    );
}

#[test]
fn user_iteration_rejects_shared_or_invalid_protocol() {
    reject(
        &format!("{COUNTER}\nmut c = Counter(0, 2)\nfor n in &c:\n    pass"),
        "exclusive source",
    );
    reject(
        "class Bad:\n    def next(self: &Bad) -> Option[i64]:\n        Nothing\nfor n in Bad():\n    pass",
        "next(self: &mut Self)",
    );
    reject(
        "class Bad:\n    def next(self: &mut Bad) -> i64:\n        0\nfor n in Bad():\n    pass",
        "must return Option",
    );
}

#[test]
fn cursor_loops_retain_the_owner_and_receiver_on_every_exit_path() {
    for (body, expected) in [
        ("values.append(9).unwrap()", "borrow"),
        ("drop(values)", "moved"),
        ("drop(cursor)", "moved"),
        ("drop(next(&mut cursor))", "borrow"),
        (
            "alias = &mut cursor\n    print(alias.position).unwrap()",
            "borrow",
        ),
    ] {
        reject(
            &format!(
                "{CURSOR}\nmut values = [2, 4].unwrap()\nmut cursor = Cursor(&values, 0)\nfor value in &mut cursor:\n    {body}\n    print(*value).unwrap()"
            ),
            expected,
        );
    }
    run(
        &format!(
            "{CURSOR}\nmut values = [2, 4, 6].unwrap()\nmut cursor = Cursor(&values, 0)\nfor value in &mut cursor:\n    if *value == 2:\n        continue\n    print(*value).unwrap()\n    break\nprint(*next(cursor).unwrap()).unwrap()\nvalues.append(8).unwrap()\nprint(len(values)).unwrap()"
        ),
        "4\n6\n4\n",
    );
}

#[test]
fn loop_items_cannot_remain_live_across_another_advance() {
    // A holder retains a result from the preceding runtime iteration, even
    // though the loop's same static next call produces every result.
    reject(
        &format!(
            "{CURSOR}\nvalues = [2, 4].unwrap()\nmut cursor = Cursor(&values, 0)\nmut previous: Option[&i64] = Nothing\nfor value in &mut cursor:\n    print(previous).unwrap()\n    previous = Some(value)\nprint(previous).unwrap()"
        ),
        "borrow",
    );
    reject(
        &format!(
            "{CURSOR}\nvalues = [2, 4].unwrap()\nmut cursor = Cursor(&values, 0)\nmut previous: Option[&i64] = Nothing\nfor value in &mut cursor:\n    previous = Some(value)\n    continue\nprint(previous).unwrap()"
        ),
        "borrow",
    );
    // Early exit leaves no next iteration, so the item may remain borrowed.
    run(
        &format!(
            "{CURSOR}\nvalues = [2, 4].unwrap()\nmut cursor = Cursor(&values, 0)\nmut first: Option[&i64] = Nothing\nfor value in &mut cursor:\n    first = Some(value)\n    break\nprint(first).unwrap()\nprint(*next(cursor).unwrap()).unwrap()"
        ),
        "Option[&i64].Some(2)\n4\n",
    );
}

#[test]
fn source_factories_temporaries_and_generic_helpers_are_evaluated_once() {
    run(
        &format!(
            "{CURSOR}\ndef cursor[T](data: &[T]) -> Cursor[T]:\n    print(100).unwrap()\n    Cursor(data, 0)\ndef consume[C](source: C) -> ():\n    for item in source:\n        print(*item).unwrap()\nvalues = [2, 4].unwrap()\nconsume(cursor(&values))\nfor item in Cursor(&[5, 7].unwrap(), 0):\n    print(*item).unwrap()"
        ),
        "100\n2\n4\n5\n7\n",
    );
}

#[test]
fn borrowed_item_pipeline_consumers_preserve_streaming_loans() {
    run(
        &format!(
            "{CURSOR}\nvalues = [2, 4].unwrap()\nmut cursor = Cursor(&values, 0)\nfor index, value in enumerate(&mut cursor):\n    print(index, *value).unwrap()\nmut second = Cursor(&values, 0)\nprint([*item for item in &mut second].unwrap()).unwrap()"
        ),
        "0 2\n1 4\n[2, 4]\n",
    );
    reject(
        &format!(
            "{CURSOR}\nvalues = [2, 4].unwrap()\nmut cursor = Cursor(&values, 0)\nmut p = iter(&mut cursor)\na = next(p).unwrap()\nb = next(p).unwrap()\nprint(a, b).unwrap()"
        ),
        "borrow",
    );
}

#[test]
fn exclusive_items_mutate_the_original_data_without_exposing_aliases() {
    let cursor = CURSOR
        .replace("data: &[T]", "data: &mut [T]")
        .replace("Option[&T]", "Option[&mut T]")
        .replace("Some(&self.data[index])", "Some(&mut self.data[index])");
    run(
        &format!(
            "{cursor}\nmut values = [2, 4].unwrap()\nmut cursor = Cursor(&mut values, 0)\nfor item in &mut cursor:\n    *item = 8\nprint(values).unwrap()"
        ),
        "[8, 8]\n",
    );
    reject(
        &format!(
            "{cursor}\nmut values = [2, 4].unwrap()\nmut cursor = Cursor(&mut values, 0)\na = next(cursor).unwrap()\nb = next(cursor).unwrap()\n*a = 8\n*b = 9"
        ),
        "borrow",
    );
}

#[test]
fn owned_item_tuples_unpack_and_cleanup_at_early_exit() {
    run(
        r#"
class Item:
    value: i64
    def __del__(self: &mut Item) -> ():
        print(self.value).unwrap()
class Items:
    position: i64
    def next(self: &mut Items) -> Option[Item]:
        if self.position >= 2:
            return Nothing
        unchecked:
            self.position += 1
        Some(Item(self.position))
    def __del__(self: &mut Items) -> ():
        print(99).unwrap()
for item in Items(0):
    print(item.value).unwrap()
    break
class Pairs:
    done: bool
    def next(self: &mut Pairs) -> Option[(i64, i64)]:
        if self.done:
            return Nothing
        self.done = True
        Some((3, 5))
for a, b in Pairs(False):
    print(a, b).unwrap()
"#,
        "1\n1\n99\n3 5\n",
    );
}

#[test]
fn loop_owned_iterators_cannot_lend_items_past_their_cleanup() {
    reject(
        &format!(
            "{CURSOR}\nvalues = [2, 4].unwrap()\nmut saved: Option[&i64] = Nothing\nfor item in Cursor(&values, 0):\n    saved = Some(item)\n    break\nprint(saved).unwrap()"
        ),
        "borrow",
    );
    reject(
        &format!(
            "{CURSOR}\nvalues = [2, 4].unwrap()\nmut cursor = Cursor(&values, 0)\nitems = [item for item in &mut cursor].unwrap()\nprint(items).unwrap()"
        ),
        "references cannot be stored",
    );
}

#[test]
fn user_iterator_sources_in_generators_and_closures_obey_existing_boundaries() {
    run(
        &format!(
            "{COUNTER}\ndef values() -> Generator[i64]:\n    for n in Counter(0, 3):\n        yield n\nprint([n for n in values()].unwrap()).unwrap()\nf = def () -> i64:\n    mut sum = 0\n    for n in Counter(0, 3):\n        unchecked:\n            sum += n\n    sum\nprint(f()).unwrap()"
        ),
        "[0, 1, 2]\n3\n",
    );
    reject(
        &format!(
            "{CURSOR}\ndef values(data: &[i64]) -> Generator[i64]:\n    for item in Cursor(data, 0):\n        yield *item\nxs = [2, 4].unwrap()\ndrop(values(&xs))"
        ),
        "yield",
    );
    reject(
        &format!(
            "{CURSOR}\nvalues = [2, 4].unwrap()\nmut cursor = Cursor(&values, 0)\ndrop(iter(&mut cursor).generator())"
        ),
        "generator",
    );
}

#[test]
fn pipeline_lending_pairing_and_generator_conversion_keep_contracts() {
    run(
        &format!(
            "{COUNTER}\nmut g = iter(Counter(0, 3)).generator()\nprint(next(g)).unwrap()\nfor n in &mut g:\n    print(n).unwrap()\nmut c = Counter(0, 2)\nfor a, b in zip(&mut c, Counter(4, 6)):\n    print(a, b).unwrap()"
        ),
        "Option[i64].Some(0)\n1\n2\n0 4\n1 5\n",
    );
    run(
        &format!(
            "{CURSOR}\nxs = [2, 4].unwrap()\nmut a = Cursor(&xs, 0)\nmut b = Cursor(&xs, 0)\nfor x, y in zip(&mut a, &mut b):\n    print(*x, *y).unwrap()\nmut c = Cursor(&xs, 0)\nmut p = iter(&mut c)\nprint(*next(p).unwrap()).unwrap()\nprint(*next(p).unwrap()).unwrap()\nprint(next(p).is_some()).unwrap()"
        ),
        "2 2\n4 4\n2\n4\nFalse\n",
    );
    reject(
        &format!(
            "{CURSOR}\nxs = [2, 4].unwrap()\nmut c = Cursor(&xs, 0)\nfor a, b in zip(&mut c, &mut c):\n    print(a, b).unwrap()"
        ),
        "borrow",
    );
}

#[test]
fn view_and_view_class_items_keep_the_exclusive_iterator_borrow() {
    let cursor = r#"
class Part:
    data: &[i64]
class Parts:
    data: &[i64]
    done: bool
    def next(self: &mut Parts) -> Option[Part]:
        if self.done:
            return Nothing
        self.done = True
        Some(Part(self.data))
"#;
    run(
        &format!(
            "{cursor}\nxs = [2, 4].unwrap()\nfor part in Parts(&xs, False):\n    print(part.data[0]).unwrap()"
        ),
        "2\n",
    );
    reject(
        &format!(
            "{cursor}\nxs = [2, 4].unwrap()\nmut p = Parts(&xs, False)\na = next(p).unwrap()\ndrop(next(p))\nprint(a.data[0]).unwrap()"
        ),
        "borrow",
    );
    let views = cursor
        .replace("Option[Part]", "Option[&[i64]]")
        .replace("Some(Part(self.data))", "Some(self.data)");
    run(
        &format!(
            "{views}\nxs = [2, 4].unwrap()\nfor part in Parts(&xs, False):\n    print(part[0]).unwrap()"
        ),
        "2\n",
    );
    reject(
        &format!(
            "{views}\nxs = [2, 4].unwrap()\nmut p = Parts(&xs, False)\na = next(p).unwrap()\ndrop(next(p))\nprint(a[0]).unwrap()"
        ),
        "borrow",
    );
}

#[test]
fn skipped_polls_preserve_retained_item_loans() {
    run(
        &format!(
            "{CURSOR}\nxs = [2, 4].unwrap()\nmut a = Cursor(&xs, 0)\nmut b = Cursor(&xs, 0)\nfor n in iter(&mut a).take(1):\n    print(*n).unwrap()\nfor n in &mut a:\n    print(*n).unwrap()\nfor n in &mut b:\n    print(*n).unwrap()"
        ),
        "2\n4\n2\n4\n",
    );
    for operation in ["drop(next(p))", "c.position = 0"] {
        reject(
            &format!(
                "{CURSOR}\nxs = [2, 4].unwrap()\nmut c = Cursor(&xs, 0)\nmut p = iter(&mut c).take(1)\na = next(p).unwrap()\n{operation}\nprint(*a).unwrap()"
            ),
            "borrow",
        );
    }
    reject(
        &format!(
            "{CURSOR}\nmut xs = [2, 4].unwrap()\nmut c = Cursor(&xs, 0)\nmut p = iter(&mut c).take(1)\na = next(p).unwrap()\nxs.append(9).unwrap()\ndrop(next(p))\nprint(*a).unwrap()"
        ),
        "borrow",
    );
    reject(
        &format!(
            "{CURSOR}\nxs = [2, 4].unwrap()\nmut a = Cursor(&xs, 0)\nmut b = Cursor(&xs, 0)\nmut saved: Option[&i64] = Nothing\nfor x, y in zip(&mut a, &mut b):\n    print(saved).unwrap()\n    saved = Some(y)\nprint(saved).unwrap()"
        ),
        "borrow",
    );
}

#[test]
fn chain_requires_owned_items_from_user_iterators() {
    run(
        &format!(
            "{COUNTER}\nfor n in iter(Counter(0, 2)).chain(Counter(2, 4)):\n    print(n).unwrap()"
        ),
        "0\n1\n2\n3\n",
    );
    reject(
        &format!(
            "{CURSOR}\nxs = [2, 4].unwrap()\nmut a = Cursor(&xs, 0)\nmut b = Cursor(&xs, 0)\nfor n in iter(&mut a).chain(&mut b):\n    print(*n).unwrap()"
        ),
        "chain cannot combine strict streaming",
    );
}

#[test]
fn wrapped_source_and_managed_view_items_keep_full_records_and_cleanup() {
    run(
        &format!(
            "{CURSOR}\ndef cursor(data: &[i64]) -> Result[Cursor[i64], str]:\n    Ok(Cursor(data, 0))\nfor n in cursor(&[7, 9].unwrap()).unwrap():\n    print(*n).unwrap()"
        ),
        "7\n9\n",
    );
    run(
        r#"
class Item:
    value: i64
    def __del__(self: &mut Item) -> ():
        print(self.value).unwrap()
class Part:
    data: &[i64]
    state: array[Item, 2]
class Parts:
    data: &[i64]
    done: bool
    def next(self: &mut Parts) -> Option[Part]:
        if self.done:
            return Nothing
        self.done = True
        Some(Part(self.data, [Item(11), Item(13)]))
xs = [2, 4].unwrap()
for part in Parts(&xs, False):
    print(part.data[0], part.state[-1].value).unwrap()
    break
print(99).unwrap()
"#,
        "2 13\n11\n13\n99\n",
    );
    reject(
        &format!(
            "{CURSOR}\nxs = [2, 4].unwrap()\nmut c = Cursor(&xs, 0)\nmut saved: Option[&i64] = Nothing\nfor n in range(2):\n    print(saved).unwrap()\n    for x in &mut c:\n        saved = Some(x)\n        break\nprint(saved).unwrap()"
        ),
        "borrow",
    );
}
