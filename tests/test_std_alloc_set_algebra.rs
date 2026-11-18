//! Compiled calls exercise the typed std wrappers and native dispatch together.
use super::test_std_alloc::assert_stdout;
use crate::test_support::check_error;

const POLICY: &str = r#"import std.alloc as alloc

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
        ("out.symmetric_difference(&a, d.keys())", "KeyView"),
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
