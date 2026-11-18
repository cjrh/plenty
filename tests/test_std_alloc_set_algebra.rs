//! Compiled calls exercise the typed std wrappers and native dispatch together.
use super::test_std_alloc::assert_stdout;
use crate::test_support::check_error;

pub(super) const POLICY: &str = r#"import std.alloc as alloc

pub class Budget:
    root: alloc.Allocator
    name: str
    left: i64
    releases: i64
    error: AllocError
    pub def allocate(self: &mut Budget, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        if self.left == 0:
            print("refused", self.name).unwrap()
            return Err(self.error)
        unchecked:
            self.left -= 1
        self.root.block(request)
    pub def release(self: &mut Budget, storage: alloc.ByteBlock) -> ():
        unchecked:
            self.releases += 1
        drop(storage)
    def __del__(self: &mut Budget) -> ():
        print(self.name, self.left, self.releases).unwrap()

def budget(name: str, blocks: i64, error: AllocError = AllocError.OutOfMemory) -> alloc.Allocator:
    root = alloc.system()
    alloc.allocator(&root, Budget(root.share(), name, blocks, 0, error)).unwrap()
"#;

#[test]
fn all_four_selected_operations_on_hash_and_bitset_sources() {
    assert_stdout(
        r#"import std.alloc as alloc

@copy
enum Flag:
    A
    B
    C

class Key:
    id: i64

def check[K: KeyType](out: &alloc.Allocator, left: &set[K], right: &set[K], a: K, b: K, c: K) -> Result[(), AllocError | IoError]:
    u = out.union(left, right)?
    i = out.intersection[K](left, right)?
    d = out.difference(left, right)?
    s = out.symmetric_difference[K](left, right)?
    print(a in u, b in u, c in u, a in i, b in i, c in i)?
    print(a in d, b in d, c in d, a in s, b in s, c in s)?
    drop(u)
    drop(i)
    drop(d)
    drop(s)
    print(len(left), a in left, b in left, len(right), b in right, c in right)?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    left = {Flag.A, Flag.B}?
    right = {Flag.B, Flag.C}?
    check(&out, &left, &right, Flag.A, Flag.B, Flag.C)?
    check(&out, {1, 2}?, {2, 3}?, 1, 2, 3)?
    check(&out, {Key(1), Key(2)}?, {Key(2), Key(3)}?, Key(1), Key(2), Key(3))?
    Ok(())
"#,
        &"True True True False True False\nTrue False False True False True\n2 True True 2 True True\n".repeat(3),
    );
}

#[test]
fn affine_hash_keys_survive_source_release_and_output_provider_failure_on_worker() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Key:
    id: i64
    label: str

def source(name: str, id: i64) -> Result[set[Key], AllocError]:
    owner = budget(name, 5)
    mut keys = owner.set_with_capacity[Key](2)?
    keys.add(Key(id, ("not a static " + "string")?))?
    keys.add(Key(0, "common"))?
    Ok(keys)

def work(out: alloc.Allocator, left: set[Key], right: set[Key]) -> Result[set[Key], AllocError | IoError]:
    partial = budget("partial", 3, AllocError.CapacityOverflow)
    match partial.union(&left, &right):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
            print("unexpected success")?
    drop(partial)
    keys = out.union[Key](&left, &right)?
    probe = Key(0, "common")
    print(len(left), probe in left, len(right), probe in right)?
    drop(left)
    drop(right)
    drop(out)
    Ok(keys)

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    out = budget("output", 5)
    a = source("left", 1)?
    b = source("right", 2)?
    with spawn(work, out, a, b).drop_job()? as task:
        mut result = task.join()?
        probe = Key(2, "not a static string")
        print(len(result), probe in result)?
        print(result.reserve(100).is_err())?
        drop(result)
    Ok(())
"#
        ),
        "refused partial\nAllocError.CapacityOverflow\npartial 0 3\n2 True 2 True\nleft 0 5\nright 0 5\n3 True\nrefused output\nTrue\noutput 0 5\n",
    );
}

#[test]
fn bitset_output_rejection_is_fallible_and_preserves_both_sources() {
    assert_stdout(
        &format!(
            r#"{POLICY}
@copy
enum Flag:
    A
    B

def main() -> Result[(), AllocError | IoError]:
    out = budget("mask", 0)
    a = {{Flag.A, Flag.B}}?
    b = {{Flag.B}}?
    match out.difference(&a, &b):
        case Err(error):
            print(error)?
        case Ok(value):
            drop(value)
    print(len(a), Flag.A in a, len(b), Flag.B in b)?
    Ok(())
"#
        ),
        "refused mask\nAllocError.OutOfMemory\n2 True 1 True\nmask 0 0\n",
    );
}

const METHODS: [&str; 4] = [
    "union",
    "intersection",
    "difference",
    "symmetric_difference",
];

#[test]
fn selected_operations_reject_nested_custom_cleanup_and_private_intrinsic_calls() {
    for method in METHODS {
        let error = check_error(&format!(
            r#"import std.alloc as alloc
class Leaf:
    id: i64
    def __del__(self) -> ():
        pass
class Key:
    value: Leaf

def combine[K: KeyType](out: &alloc.Allocator, a: &set[K]) -> Result[set[K], AllocError]:
    out.{method}(a, a)

def main() -> ():
    out = alloc.system()
    a = set[Key]().unwrap()
    drop(combine(&out, &a))
"#
        ));
        assert!(
            error.contains("requires copying keys") && error.contains("custom cleanup"),
            "{method}: {error}"
        );
        let error = check_error(&format!(
            "def main() -> ():\n    drop(__allocator_set_{method}[set[i64]](0, 0, 0))\n"
        ));
        assert!(
            error.contains("reserved for the standard library"),
            "{method}: {error}"
        );
    }
}

#[test]
fn selected_set_algebra_requires_matching_legal_sets() {
    for (call, diagnostic) in [
        ("out.union(&a, &b)", "conflicting types for `K`"),
        (
            "out.intersection[str](&a, &b)",
            "expected &set[str], got &set[i64]",
        ),
        (
            "out.symmetric_difference(&b, d.keys())",
            "conflicting types for `K`",
        ),
        ("out.union(d.keys(), &b)", "conflicting types for `K`"),
        (
            "out.difference[str](d.keys(), &b)",
            "expected KeyView[str], got KeyView[i64]",
        ),
        ("out.intersection(d.keys(), &d.keys())", "&KeyView"),
        ("out.union(d.keys(), d.values())", "ValueView"),
        (
            "out.difference[Hidden](&a, &a)",
            "field channel: Sender[i64] is not a supported key type",
        ),
    ] {
        let error = check_error(&format!(
            r#"import std.alloc as alloc
class Hidden:
    channel: Sender[i64]
def main() -> ():
    out = alloc.system()
    a = set[i64]().unwrap()
    b = set[str]().unwrap()
    d = dict[i64, str]().unwrap()
    drop({call})
"#
        ));
        assert!(error.contains(diagnostic), "{call}: {error}");
    }
}

#[test]
fn selected_set_algebra_excludes_actors_even_in_unused_bodies() {
    for method in METHODS {
        let error = check_error(&format!(
            "import std.alloc as alloc\nimport std.actor as actors\ndef unused(out: &alloc.Allocator, a: &set[str]) -> ():\n    drop(out.{method}(a, a))\ndef main() -> ():\n    drop(actors.heaps_live())\n"
        ));
        assert!(
            error.contains("retained allocator use cannot be combined"),
            "{method}: {error}"
        );
    }
}

#[test]
fn worker_algebra_borrows_both_sources_for_the_task_scope() {
    for target in ["left", "right"] {
        let error = check_error(&format!(
            r#"import std.alloc as alloc

def work(out: &alloc.Allocator, left: &set[i64], right: &set[i64]) -> Result[set[i64], AllocError]:
    out.intersection(left, right)

def main() -> ():
    out = alloc.system()
    mut left = {{1}}.unwrap()
    mut right = {{2}}.unwrap()
    with spawn(work, &out, &left, &right).drop_job().unwrap() as task:
        {target}.add(1).unwrap()
        drop(task.join())
"#
        ));
        assert!(
            error.contains(&format!("cannot modify or exclusively borrow `{target}`")),
            "{error}"
        );
    }
}

#[test]
fn mixed_algebra_membership_and_argument_order() {
    // All four operations in all three mixed shapes; view/view does not
    // require the two dictionaries to have the same value type.
    let mut calls = String::new();
    let mut expected = String::new();
    for (method, members) in METHODS.into_iter().zip([
        "True True True",
        "False True False",
        "True False False",
        "True False True",
    ]) {
        for (left, right) in [
            ("view", "&b"),
            ("&a", "right.keys()"),
            ("left.keys()", "right.keys()"),
        ] {
            calls += &format!("    show(out.{method}({left}, {right})?)?\n");
            expected += members;
            expected += "\n";
        }
    }
    assert_stdout(
        &format!(
            r#"import std.alloc as alloc
class Player:
    score: i64
    def __del__(self) -> ():
        pass

def show(keys: &set[i64]) -> Result[(), IoError]:
    print(1 in keys, 2 in keys, 3 in keys)

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    a = {{1, 2}}?
    b = {{2, 3}}?
    left = {{1: Player(11), 2: Player(22)}}?
    right = {{2: "yes", 3: "no"}}?
    view = left.keys()
{calls}    show(out.difference[i64](right=view, left=&b)?)?
    show(out.difference(right=&b, left=view)?)?
    print(len(view), left[1].score)?
    Ok(())
"#
        ),
        &(expected + "False False True\nTrue False False\n2 11\n"),
    );
}

#[test]
fn reversed_keyword_difference_keeps_temporary_dictionaries_alive() {
    let stdout = crate::test_support::run_stdout(
        r#"import std.alloc as alloc
class Payload:
    side: str
    key: i64
    def __del__(self) -> ():
        print("drop", self.side, self.key).unwrap()

def source(side: str, key: i64) -> Result[dict[i64, Payload], AllocError | IoError]:
    print("eval", side)?
    Ok({key: Payload(side, key), 2: Payload(side, 2)}?)

def show(keys: &set[i64]) -> Result[(), IoError]:
    print(1 in keys, 2 in keys, 3 in keys)

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    show(out.difference(right=source("right", 3)?.keys(), left=source("left", 1)?.keys())?)?
    data = {1: "one"}?
    view = data.keys()
    result = out.difference(view, data.keys())?
    print(len(result), 1 in view, data[1])?
    Ok(())
"#,
    );
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("eval right"));
    assert_eq!(lines.next(), Some("eval left"));
    assert_eq!(lines.next(), Some("True False False"));
    assert_eq!(lines.next_back(), Some("0 True one"));
    // Both owners live through show's full expression. Only the cleanup
    // multiset matters, not which owner or dictionary entry drops first.
    let mut cleanup: Vec<_> = lines.collect();
    cleanup.sort_unstable();
    assert_eq!(
        cleanup,
        ["drop left 1", "drop left 2", "drop right 2", "drop right 3"]
    );
}

#[test]
fn selected_provider_refusal_preserves_affine_key_sources_and_result_outlives_both() {
    assert_stdout(
        &format!(
            r#"{POLICY}
class Key:
    id: i64
    name: str
class Player:
    score: i64
    def __del__(self) -> ():
        print(self.score).unwrap()

def main() -> Result[(), AllocError | IoError]:
    mut invited = {{Key(1, ("dy" + "namic")?): Player(17)}}?
    mut replied = {{Key(2, "other")}}?
    partial = budget("partial", 3, AllocError.CapacityOverflow)
    view = invited.keys()
    match partial.difference(view, &replied):
        case Err(error):
            print(error)?
        case Ok(unexpected):
            drop(unexpected)
    drop(partial)
    out = budget("out", 5)
    mut result = out.difference[Key](view, &replied)?
    print(len(view), len(replied))?
    invited.clear()
    replied.clear()
    drop(invited)
    drop(replied)
    drop(out)
    print(Key(1, "dynamic") in result, result.reserve(100).is_err())?
    Ok(())
"#
        ),
        "refused partial\nAllocError.CapacityOverflow\npartial 0 3\n1 1\n17\nrefused out\nTrue True\nout 0 5\n",
    );
}

#[test]
fn mixed_algebra_rejects_nonduplicable_dictionary_key_in_generic_code() {
    let error = check_error(
        r#"import std.alloc as alloc
class Leaf:
    id: i64
    def __del__(self) -> ():
        pass
class Key:
    leaf: Leaf

def work[K: KeyType](out: &alloc.Allocator, left: &dict[K, i64], right: &set[K]) -> Result[set[K], AllocError]:
    out.difference(left.keys(), right)

def main() -> ():
    out = alloc.system()
    left = dict[Key, i64]().unwrap()
    right = set[Key]().unwrap()
    drop(work(&out, &left, &right))
"#,
    );
    assert!(
        error.contains("requires copying keys") && error.contains("custom cleanup"),
        "{error}"
    );
}

#[test]
fn first_dictionary_view_stays_borrowed_while_evaluating_second_source() {
    let error = check_error(
        r#"import std.alloc as alloc
def clear(items: &mut dict[i64, bool]) -> set[i64]:
    items.clear()
    set[i64]().unwrap()

def main() -> ():
    out = alloc.system()
    mut data = {1: True}.unwrap()
    drop(out.union(data.keys(), clear(&mut data)))
"#,
    );
    assert!(
        error.contains("borrow") && error.contains("data"),
        "{error}"
    );
}

#[test]
fn mixed_algebra_is_not_a_general_call_parameter() {
    let fake = check_error(
        r#"class Allocator:
    def union[K: KeyType](self, left: &set[K], right: &set[K]) -> Result[set[K], AllocError]:
        left.union(right)

def main() -> ():
    items = {1: True}.unwrap()
    keys = {1}.unwrap()
    drop(Allocator().union(items.keys(), &keys))
"#,
    );
    assert!(fake.contains("KeyView"), "{fake}");
    let indirect = check_error(
        r#"def apply(op: Callable[[&set[i64], &set[i64]], ()], data: &dict[i64, bool], keys: &set[i64]) -> ():
    op(data.keys(), keys)
def main() -> ():
    pass
"#,
    );
    assert!(
        indirect.contains("expected &set[i64]") && indirect.contains("KeyView[i64]"),
        "{indirect}"
    );
}

#[test]
fn worker_cannot_capture_or_receive_algebra_view() {
    for (job, diagnostic) in [
        ("out.union, view, &keys", "takes no additional arguments"),
        (
            "def [&out, &view, &keys]() -> Result[set[i64], AllocError]: out.union(view, &keys)",
            "cannot be captured",
        ),
    ] {
        let error = check_error(&format!(
            r#"import std.alloc as alloc
def main() -> ():
    out = alloc.system()
    data = {{1: True}}.unwrap()
    keys = {{2}}.unwrap()
    view = data.keys()
    with spawn({job}).unwrap() as task:
        drop(task.join())
"#
        ));
        assert!(error.contains(diagnostic), "{job}: {error}");
    }
}

#[test]
fn worker_can_project_locally_but_must_check_the_entire_dictionary() {
    let source = r#"import std.alloc as alloc
@copy
enum Flag:
    A
    B
class Payload:
    id: i64
    def __del__(self) -> ():
        pass

def work[K: KeyType](out: &alloc.Allocator, data: &dict[K, Payload], keys: &set[K]) -> Result[set[K], AllocError]:
    out.intersection(data.keys(), keys)

def worker(out: &alloc.Allocator, data: &dict[Flag, Payload], keys: &set[Flag]) -> Result[set[Flag], AllocError]:
    work(out, data, keys)

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    data = {Flag.A: Payload(1)}?
    keys = {Flag.A, Flag.B}?
    out = alloc.system()
    with spawn(worker, &out, &data, &keys).drop_job()? as task:
        result = task.join()?
        print(len(result), Flag.A in result, Flag.B in result)?
    Ok(())
"#;
    assert_stdout(source, "1 True False\n");
    let unsafe_payload = source.replace("import std.alloc as alloc", "import std.alloc as alloc\nimport std.env")
        .replace("        pass", "        path = std.env.current_dir().unwrap()\n        std.env.set_current_dir(&path).unwrap()");
    let error = check_error(&unsafe_payload);
    assert!(error.contains("changing the working directory"), "{error}");
}
