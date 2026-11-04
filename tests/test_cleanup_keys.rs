//! Cleanup-bearing keys can transfer or borrow, but never duplicate owners.
mod support;
use support::{assert_visible_stdout as native, check_error};

const KEY: &str = r#"
class Key:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
"#;

#[test]
fn cleanup_keys_borrow_replace_and_remove() {
    native(
        &format!(
            r#"{KEY}
mut data = {{Key(1): 10}}.unwrap()
probe = Key(1)
print(probe in data, data.get(&probe).unwrap()).unwrap()
data.insert(Key(1), 20).unwrap()
print(data[&probe]).unwrap()
match data.get_mut(&probe):
    case Some(value):
        *value += 1
    case Nothing:
        pass
for key, value in data.items():
    print(key.id, *value).unwrap()
print(data.pop(&probe).unwrap(), len(data)).unwrap()
print("probe alive", probe.id).unwrap()
drop(probe)
mut keys = {{Key(2)}}.unwrap()
keys.add(Key(2)).unwrap()
probe2 = Key(2)
print(keys.discard(&probe2), keys.discard(&probe2)).unwrap()
drop(probe2)
"#
        ),
        "True 10\ndrop 1\n20\n1 21\ndrop 1\n21 0\nprobe alive 1\ndrop 1\ndrop 2\ndrop 2\nTrue False\ndrop 2",
    );
}

#[test]
fn cleanup_keys_transfer_updates_and_compare_views() {
    native(
        &format!(
            r#"{KEY}
mut keys = {{Key(1)}}.unwrap()
keys.update({{Key(1), Key(2)}}.unwrap()).unwrap()
mut data = {{Key(1): 10, Key(2): 20}}.unwrap()
print(keys == data.keys(), keys.issubset(data.keys()), data.keys().issuperset(keys), keys.isdisjoint(data.keys())).unwrap()
keep = {{Key(2): True}}.unwrap()
keys.intersection_update(keep.keys())
keys.difference_update(keep.keys())
print(len(keys)).unwrap()
data.update({{Key(2): 22, Key(3): 30}}.unwrap()).unwrap()
print("extract").unwrap()
owned = data.into_keys().unwrap()
for key in &owned:
    print(key.id).unwrap()
drop(owned)
drop(keep)
"#
        ),
        "drop 1\nTrue True True False\ndrop 1\ndrop 2\n0\ndrop 2\nextract\n1\n2\n3\ndrop 1\ndrop 2\ndrop 3\ndrop 2",
    );
}

#[test]
fn cleanup_keys_nested_and_generic_transfer() {
    native(
        &format!(
            r#"{KEY}
enum Entry:
    Empty
    Held(Key)
class Wrapped:
    value: Entry
def collect[K: KeyType](items: list[K]) -> Result[set[K], AllocError]:
    set(items)
def inspect[K: KeyType](items: &set[K]) -> bool:
    items.issubset(items)
keys = collect([Wrapped(Entry.Held(Key(1))), Wrapped(Entry.Held(Key(1)))].unwrap()).unwrap()
print(len(keys), inspect(keys)).unwrap()
drop(keys)
mut sums: set[Key | str] = {{Key(2), "two"}}.unwrap()
probe = Key(2)
print(probe in sums).unwrap()
print(sums.discard(&probe)).unwrap()
drop(probe)
pairs = {{(Key(3), True)}}.unwrap()
for pair in pairs:
    drop(pair)
owned = {{Key(4): False}}.unwrap().keys_snapshot().unwrap()
print(owned[0].id).unwrap()
drop(owned)
"#
        ),
        "drop 1\n1 True\ndrop 1\nTrue\ndrop 2\nTrue\ndrop 2\ndrop 3\n4\ndrop 4",
    );
}

#[test]
fn cleanup_keys_reject_all_copying_algebra_and_materialization() {
    for ty in ["Key", "(Key, bool)", "Entry", "Key | str", "Wrapped"] {
        for lhs in ["a", "d.keys()"] {
            for rhs in ["b", "e.keys()"] {
                for method in [
                    "union",
                    "intersection",
                    "difference",
                    "symmetric_difference",
                ] {
                    let error = check_error(&format!(
                        r#"{KEY}
enum Entry:
    Empty
    Held(Key)
class Wrapped:
    value: Key
def bad(a: &set[{ty}], b: &set[{ty}], d: &dict[{ty}, i64], e: &dict[{ty}, bool]) -> ():
    drop({lhs}.{method}({rhs}))
bad(set[{ty}]().unwrap(), set[{ty}]().unwrap(), dict[{ty}, i64]().unwrap(), dict[{ty}, bool]().unwrap())
"#
                    ));
                    assert!(
                        error.contains("requires copying keys") && error.contains("custom cleanup"),
                        "{error}"
                    );
                }
            }
        }
        let error = check_error(&format!(
            r#"{KEY}
enum Entry:
    Empty
    Held(Key)
class Wrapped:
    value: Key
d = dict[{ty}, i64]().unwrap()
drop(d.keys().to_set())
"#
        ));
        assert!(error.contains("to_set requires copying keys"), "{error}");
    }
    for expression in ["copy(s)", "copy(d)", "copy(key)"] {
        let error = check_error(&format!(
            "{KEY}\ns = set[Key]().unwrap()\nd = dict[Key, i64]().unwrap()\nkey = Key(1)\ndrop({expression})\n"
        ));
        assert!(error.contains("cannot be copied"), "{error}");
    }
    let error = check_error(&format!(
        "{KEY}\ndef combine[K: KeyType](a: &set[K]) -> Result[set[K], AllocError]:\n    a.union(a)\ndrop(combine(set[Key]().unwrap()))\n"
    ));
    assert!(error.contains("requires copying keys"), "{error}");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn cleanup_keys_allocation_failure_transfers_or_destroys_once() {
    // Reset allocation injection inside cleanup so observing the destructor
    // cannot itself fail. No further allocation is needed after a failed reserve.
    let key = KEY.replace(
        "        print(\"drop\"",
        "        print(\"__test_restore_allocations__\").unwrap()\n        print(\"drop\"",
    );
    for budget in 0..=2 {
        native(
            &format!(
                r#"{key}
data = {{Key(1): 10, Key(2): 20}}.unwrap()
print("__test_fail_allocations_after_{budget}__").unwrap()
result = data.into_keys()
print("__test_restore_allocations__").unwrap()
print(result.is_ok()).unwrap()
drop(result)
"#
            ),
            if budget < 2 {
                "drop 1\ndrop 2\nFalse"
            } else {
                "True\ndrop 1\ndrop 2"
            },
        );
    }
    for operation in ["keys.add(Key(2))", "keys.update({Key(2)}.unwrap())"] {
        // Build update source before activating failure injection.
        let (setup, operation) = if operation.contains("update") {
            ("incoming = {Key(2)}.unwrap()", "keys.update(incoming)")
        } else {
            ("", operation)
        };
        native(
            &format!(
                r#"{key}
mut keys = set[Key]().unwrap()
{setup}
print("__test_fail_allocations_after_0__").unwrap()
result = {operation}
print("__test_restore_allocations__").unwrap()
print(result.is_err(), len(keys)).unwrap()
"#
            ),
            "drop 2\nTrue 0",
        );
    }
}

#[test]
fn cleanup_keys_owned_iteration_drops_unvisited_entries_and_values() {
    native(
        &format!(
            r#"{KEY}
keys = {{Key(1), Key(2), Key(3)}}.unwrap()
for key in keys:
    print("take", key.id).unwrap()
    break
print("dictionary").unwrap()
data = {{Key(4): Key(40), Key(5): Key(50)}}.unwrap()
for key in data:
    print("take", key.id).unwrap()
    break
"#
        ),
        "take 1\ndrop 1\ndrop 2\ndrop 3\ndictionary\ntake 4\ndrop 4\ndrop 40\ndrop 5\ndrop 50",
    );
}

#[test]
fn cleanup_key_observations_keep_loans_and_temporary_owners() {
    native(
        &format!(
            r#"{KEY}
mut keys = {{Key(1)}}.unwrap()
print(keys.discard(Key(1))).unwrap()
mut data = {{Key(2): 20}}.unwrap()
match data.get_mut(Key(2)):
    case Some(value):
        *value += 1
    case Nothing:
        pass
print(data.pop(Key(2)).unwrap()).unwrap()
"#
        ),
        "drop 1\nTrue\ndrop 1\ndrop 2\ndrop 2\n21\ndrop 2",
    );
    for action in ["drop(keys.discard(probe))", "keys.clear()"] {
        let error = check_error(&format!(
            r#"{KEY}
mut keys = {{Key(1)}}.unwrap()
for probe in &keys:
    {action}
"#
        ));
        assert!(error.contains("borrow"), "{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn cleanup_keys_insertion_and_updates_handle_every_growth_failure() {
    let key = KEY.replace(
        "        print(\"drop\"",
        "        print(\"__test_restore_allocations__\").unwrap()\n        print(\"drop\"",
    );
    for budget in 0..=3 {
        for (setup, operation) in [
            ("", "data.insert(Key(1), Key(10))"),
            (
                "incoming = {Key(1): Key(10)}.unwrap()",
                "data.update(incoming)",
            ),
        ] {
            native(
                &format!(
                    r#"{key}
mut data = dict[Key, Key]().unwrap()
{setup}
print("__test_fail_allocations_after_{budget}__").unwrap()
result = {operation}
print("__test_restore_allocations__").unwrap()
print(result.is_ok(), len(data)).unwrap()
drop(data)
"#
                ),
                if budget < 3 {
                    "drop 1\ndrop 10\nFalse 0"
                } else {
                    "True 1\ndrop 1\ndrop 10"
                },
            );
        }
    }
}

#[test]
fn cleanup_key_snapshots_recommend_transfer_instead_of_copy() {
    let error = check_error(&format!(
        "{KEY}\ndata = {{Key(1): 1}}.unwrap()\ndrop(data.keys_snapshot())\n"
    ));
    assert!(error.contains("use into_keys()"), "{error}");
}

#[test]
fn cleanup_keys_with_owned_strings_survive_rehash_and_transfer() {
    native(
        r#"
class Key:
    label: str
    def __del__(self) -> ():
        print("drop", self.label).unwrap()
class Wrapped:
    key: Key
    def __del__(self) -> ():
        print("outer", self.key.label).unwrap()
mut keys = set[Wrapped]().unwrap()
keys.add(Wrapped(Key(("account-" + "first").unwrap()))).unwrap()
keys.reserve(100).unwrap()
probe = Wrapped(Key(("account-" + "first").unwrap()))
print(probe in keys).unwrap()
for key in keys:
    print(key.key.label).unwrap()
drop(probe)
mut data = {Key(("account-" + "second").unwrap()): 2}.unwrap()
probe2 = Key(("account-" + "second").unwrap())
match data.get_mut(&probe2):
    case Some(value):
        *value += 1
    case Nothing:
        pass
owned = data.into_keys().unwrap()
print(owned[0].label, probe2.label).unwrap()
drop(owned)
drop(probe2)
"#,
        "True\naccount-first\nouter account-first\ndrop account-first\nouter account-first\ndrop account-first\naccount-second account-second\ndrop account-second\ndrop account-second",
    );
}
