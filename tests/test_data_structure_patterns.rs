//! Executable counterparts of the data-structure investigation and arena prototype.
mod support;

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, code) in [
        (
            "linked.plenty",
            include_str!("../examples/data_structures/linked.plenty"),
        ),
        (
            "tree.plenty",
            include_str!("../examples/data_structures/tree.plenty"),
        ),
        (
            "arena.plenty",
            include_str!("../examples/arena/arena.plenty"),
        ),
        (
            "trie.plenty",
            include_str!("../book/src/design/examples/data-structures/trie/trie.plenty"),
        ),
        (
            "btree.plenty",
            include_str!("../book/src/design/examples/data-structures/balanced-trees/btree.plenty"),
        ),
        (
            "red_black.plenty",
            include_str!(
                "../book/src/design/examples/data-structures/balanced-trees/red_black.plenty"
            ),
        ),
    ] {
        support::write_file(&dir.path().join(name), code);
    }
    dir
}

fn compile(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = workspace();
    let executable = support::compile_file(dir.path(), source);
    (dir, executable)
}

fn assert_output(output: std::process::Output, expected: &str) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim_end());
}

fn runs(source: &str, expected: &str) {
    let (_dir, executable) = compile(source);
    assert_output(
        std::process::Command::new(executable).output().unwrap(),
        expected,
    );
}

#[test]
fn trie_demo_runs() {
    runs(
        include_str!("../book/src/design/examples/data-structures/trie/main.plenty"),
        "[\"\", \"cafe\u{301}\", \"café\", \"car\", \"cart\", \"cat\", \"dog\", \"猫\", \"猫咪\"]\nTrue False\nTrue False\n[\"car\", \"cart\"]\n[\"猫\", \"猫咪\"]\nOption[str].Some(\"cart\")\nOption[str].Some(\"\")\nFalse\nTrue True\n[\"cart\"]\nTrue\nOption[str].Nothing\nTrue",
    );
}

#[test]
fn trie_matches_sorted_model_through_unicode_insert_delete_prefix_and_reuse() {
    runs(
        include_str!("../book/src/design/examples/data-structures/trie/checks.plenty"),
        "14400 permutation pairs passed\n8000 Unicode mixed operations and prefix queries passed\n256-way root/interior fanout and sibling deletion passed\n10000-scalar results survive original key, query and trie owners",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn trie_allocation_failures_preserve_keys_and_pruning_allocates_nothing() {
    runs(
        include_str!("fixtures/trie_allocation.plenty"),
        "insertion allocation-failure sweep and retry passed\nprefix-result allocation-failure sweep passed\nallocation-free lookup, pruning and reuse passed",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn trie_long_keys_use_a_small_native_stack() {
    let (_dir, executable) = compile(
        r#"
import trie
def main() -> Result[(), AllocError | IoError]:
    key = "猫".repeat(100000)?
    mut tree = trie.create()?
    drop(trie.insert(&mut tree, key)?)
    print(len(trie.ordered(&tree)?[0]))?
    print(trie.valid(&tree)?)?
    print(trie.remove(&mut tree, key))?
    print(trie.length(&tree))?
    print(trie.valid(&tree)?)?
    Ok(())
"#,
    );
    let output = std::process::Command::new("sh")
        .args(["-c", "ulimit -s 256; exec \"$1\"", "trie-stack-test"])
        .arg(executable)
        .output()
        .unwrap();
    assert_output(output, "100000\nTrue\nTrue\n0\nTrue");
}

#[test]
fn balanced_tree_demo_runs() {
    runs(
        include_str!("../book/src/design/examples/data-structures/balanced-trees/main.plenty"),
        "[10, 20, 30, 40, 50, 60, 70]\n[10, 20, 30, 40, 50, 60, 70]\nFalse False\nTrue False\nTrue True\nFalse False\n[10, 20, 30, 50, 60, 70]\n[10, 20, 30, 50, 60, 70]\nTrue True",
    );
}

#[test]
fn balanced_trees_preserve_invariants_through_insert_delete_and_reuse() {
    runs(
        include_str!("../book/src/design/examples/data-structures/balanced-trees/checks.plenty"),
        "14400 permutation pairs passed\n4 multi-level split/merge and reuse scenarios passed\n16000 mixed operations and final drains passed",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn balanced_tree_allocation_failures_preserve_sets_and_removal_allocates_nothing() {
    runs(
        include_str!("fixtures/balanced_tree_allocation.plenty"),
        "allocation failure sweeps passed\nallocation-free deletion and slot reuse passed",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn btree_capacity_is_checked_before_mutation_but_duplicates_still_succeed() {
    let dir = workspace();
    // Synthetic private state tests the representable key-count boundary without
    // pretending to allocate MAX nodes or adding a production privacy bypass.
    let instrumented = format!(
        "{}{}",
        include_str!("../book/src/design/examples/data-structures/balanced-trees/btree.plenty"),
        r#"
pub def capacity_probe() -> Result[(), AllocError | IoError]:
    mut tree = create()?
    drop(insert(&mut tree, 1)?)
    root = tree.root
    free = tree.free
    available = tree.available
    slots = len(tree.nodes)
    before = ordered(&tree)?
    tree.size = i64.MAX
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    duplicate = insert(&mut tree, 1)
    failure = insert(&mut tree, 2)
    unchanged = tree.size == i64.MAX and tree.root == root and tree.free == free and tree.available == available and len(tree.nodes) == slots
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(str.repr(duplicate)?, str.repr(failure)?, unchanged)?
    tree.size = 1
    after = ordered(&tree)?
    print(before == after, after, valid(&tree)?)?
    Ok(())
"#
    );
    support::write_file(&dir.path().join("btree.plenty"), &instrumented);
    let executable = support::compile_file(
        dir.path(),
        "import btree\ndef main() -> Result[(), AllocError | IoError]:\n    btree.capacity_probe()\n",
    );
    assert_output(
        std::process::Command::new(executable).output().unwrap(),
        "Result[bool, AllocError].Ok(False) Result[bool, AllocError].Err(AllocError.CapacityOverflow) True\nTrue [1] True",
    );
}

#[test]
fn data_structure_counter_fields_are_private_not_merely_underscored() {
    for (module, construction, field) in [
        ("arena", "arena.Arena[i64](1)?", "_count"),
        ("btree", "btree.create()?", "size"),
        ("red_black", "red_black.create()?", "size"),
        ("trie", "trie.create()?", "available"),
    ] {
        let dir = workspace();
        let source = format!(
            "import {module}\ndef main() -> Result[(), AllocError]:\n    mut tree = {construction}\n    tree.{field} = i64.MAX\n    Ok(())\n"
        );
        let path = support::write_main(dir.path(), &source);
        let error = plenty::check_file(&path, None).unwrap_err().to_string();
        assert!(error.contains("private"), "{module}.{field}: {error}");
    }
}

#[test]
fn arena_demo_runs() {
    runs(
        include_str!("../examples/arena/main.plenty"),
        "Option[bool].Some(True)\nOption[i64].Some(7)\nOption[i64].Some(23)\nOption[i64].Some(7)\nFalse\nOption[i64].Some(30)",
    );
}

#[test]
fn arena_parent_links_copy_keys_without_copying_nodes_and_detect_removed_parent() {
    runs(
        include_str!("../examples/arena/parent_links.plenty"),
        "Option[str].Some(\"root\")\nOption[str].Nothing",
    );
}

#[test]
fn arena_keys_are_typed_and_arenas_cannot_be_copied() {
    for (body, diagnostic) in [
        (
            "mut strings = Arena[str](2)?\n    key = strings.insert(\"x\")?\n    arena.contains(&key)",
            "Key",
        ),
        ("copy(arena)", "cannot be copied"),
    ] {
        let dir = workspace();
        let source = format!(
            "from arena import Arena\ndef main() -> Result[(), AllocError]:\n    mut arena = Arena[i64](1)?\n    {body}\n    Ok(())\n"
        );
        let path = support::write_main(dir.path(), &source);
        let error = plenty::check_file(&path, None).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}

#[test]
fn arena_callbacks_cannot_return_borrows_or_reenter_the_borrowed_arena() {
    // B11 may add stored borrowed results. Revisit the read callback case with
    // that contract; two-node callbacks must never lend their temporary locals.
    for (helpers, body, diagnostic) in [
        (
            "def lend(value: &i64) -> &i64:\n    value\n",
            "arena.read(&first, lend)",
            "reference",
        ),
        (
            "",
            "observe = def [&arena](a: &mut i64, b: &mut i64) -> i64:\n        arena.len()\n    arena.update_two(&first, &second, observe)",
            "borrow",
        ),
    ] {
        let dir = workspace();
        let source = format!(
            "from arena import Arena\n{helpers}def main() -> Result[(), AllocError]:\n    mut arena = Arena[i64](1)?\n    first = arena.insert(1)?\n    second = arena.insert(2)?\n    {body}\n    Ok(())\n"
        );
        let path = support::write_main(dir.path(), &source);
        let error = plenty::check_file(&path, None).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
}

#[test]
fn arena_rejects_stale_foreign_and_duplicate_keys_and_retires_exhausted_slots() {
    runs(
        r#"
from arena import Arena
def value(x: &i64) -> i64:
    *x
def increment(x: &mut i64) -> Result[i64, OverflowError[i64]]:
    *x += 1
    Ok(*x)
def add(left: &mut i64, right: &mut i64) -> Result[bool, OverflowError[i64]]:
    *left += *right
    Ok(True)
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut arena = Arena[i64](10, generation_limit=1)?
    mut other = Arena[i64](11)?
    first = arena.insert(1)?
    foreign = other.insert(99)?
    print(arena.contains(&foreign))?
    unchanged: Option[bool] = match arena.update_two(&first, &first, add):
        case Some(result):
            Some(result?)
        case Nothing:
            Nothing
    print(unchanged)?
    print(arena.remove(&first))?
    second = arena.insert(2)?
    print(arena.contains(&first))?
    print(arena.remove(&first))?
    print(arena.remove(&second))?
    third = arena.insert(3)?
    print(arena.contains(&first))?
    print(arena.contains(&second))?
    print(arena.read(&third, value))?
    updated: Option[i64] = match arena.update(&third, increment):
        case Some(result):
            Some(result?)
        case Nothing:
            Nothing
    print(updated)?
    print(arena.len())?
    Ok(())
"#,
        "False\nOption[bool].Nothing\nOption[i64].Some(1)\nFalse\nOption[i64].Nothing\nOption[i64].Some(2)\nFalse\nFalse\nOption[i64].Some(3)\nOption[i64].Some(4)\n1",
    );
}

#[test]
fn arena_restores_affine_payloads_even_when_callback_returns_error() {
    runs(
        r#"
from arena import Arena
class Item:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
enum Stopped:
    Done
def change(left: &mut Item, right: &mut Item) -> Result[bool, Stopped | OverflowError[i64]]:
    left.value += 10
    right.value += 20
    Err(Stopped.Done)
def main() -> Result[(), AllocError | IoError]:
    mut arena = Arena[Item](1)?
    first = arena.insert(Item(1))?
    second = arena.insert(Item(2))?
    print(arena.update_two(&first, &second, change))?
    print(arena.contains(&first))?
    print(arena.contains(&second))?
    drop(arena.remove(&first))
    drop(arena)
    Ok(())
"#,
        "Option[Result[bool, OverflowError[i64] | Stopped]].Some(Result[bool, OverflowError[i64] | Stopped].Err(Stopped.Done))\nTrue\nTrue\n11\n22",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn linked_arithmetic_reports_first_overflow_and_bump_keeps_only_completed_increments() {
    runs(
        r#"
from linked import Linked, push, total, bump, pop
def main() -> Result[(), AllocError | IoError]:
    mut positive = Linked(Nothing)
    for value in [-1, 1, i64.MAX]?:
        push(&mut positive, value)?
    mut negative = Linked(Nothing)
    for value in [1, -1, i64.MIN]?:
        push(&mut negative, value)?
    mut changing = Linked(Nothing)
    for value in [7, i64.MAX, 1]?:
        push(&mut changing, value)?
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    high = total(&positive)
    low = total(&negative)
    failure = bump(&mut changing)
    first = pop(&mut changing)
    second = pop(&mut changing)
    third = pop(&mut changing)
    drop(positive)
    drop(negative)
    drop(changing)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(str.repr(high)?, str.repr(low)?)?
    print(str.repr(failure)?)?
    print(first, second, third)?
    Ok(())
"#,
        "Result[i64, OverflowError[i64]].Err(OverflowError[i64](wrapped=-9223372036854775808)) Result[i64, OverflowError[i64]].Err(OverflowError[i64](wrapped=9223372036854775807))\nResult[(), OverflowError[i64]].Err(OverflowError[i64](wrapped=-9223372036854775808))\nOption[i64].Some(2) Option[i64].Some(9223372036854775807) Option[i64].Some(7)",
    );
}

#[test]
fn arena_transfer_preflights_both_values_and_restores_payloads_after_arithmetic_error() {
    let helpers = include_str!("../examples/arena/main.plenty")
        .split("def main(")
        .next()
        .unwrap();
    let source = format!(
        "{helpers}{}",
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut nodes = Arena[i64](1)?
    first = nodes.insert(10)?
    second = nodes.insert(i64.MAX)?
    print(nodes.update_two(&first, &second, transfer))?
    print(nodes.read(&first, value), nodes.read(&second, value), nodes.len())?
    print(nodes.update_two(&first, &first, transfer))?
    drop(nodes.remove(&first))
    print(nodes.update_two(&first, &second, transfer))?
    mut left = i64.MIN
    mut right = 20
    print(str.repr(transfer(&mut left, &mut right))?)?
    print(left, right)?
    left = -9223372036854775805
    right = 9223372036854775804
    print(str.repr(transfer(&mut left, &mut right))?)?
    print(left, right)?
    Ok(())
"#
    );
    runs(
        &source,
        "Option[Result[bool, OverflowError[i64]]].Some(Result[bool, OverflowError[i64]].Err(OverflowError[i64](wrapped=-9223372036854775806)))\nOption[i64].Some(10) Option[i64].Some(9223372036854775807) 2\nOption[Result[bool, OverflowError[i64]]].Nothing\nOption[Result[bool, OverflowError[i64]]].Nothing\nResult[bool, OverflowError[i64]].Err(OverflowError[i64](wrapped=9223372036854775805))\n-9223372036854775808 20\nResult[bool, OverflowError[i64]].Ok(True)\n-9223372036854775808 9223372036854775807",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_push_preserves_existing_chain_and_rotation_keeps_allocations() {
    runs(
        r#"
from linked import Linked, push, total, pop
from tree import Tree, rotate_right
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut chain = Linked(Nothing)
    push(&mut chain, 1)?
    print("__test_fail_allocations_after_0__")?
    failure = push(&mut chain, 2)
    print("__test_restore_allocations__")?
    print(str.repr(failure)?)?
    print(total(&chain)?)?
    print(pop(&mut chain))?
    print(pop(&mut chain))?
    root = Box(Tree(3, Some(Box(Tree(1, Nothing, Some(Box(Tree(2, Nothing, Nothing))?)))?), Nothing))?
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    rotated = rotate_right(root)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(rotated.value)?
    match &rotated.right:
        case Some(right):
            print(right.value)?
            match &right.left:
                case Some(middle):
                    print(middle.value)?
                case Nothing:
                    print("lost middle")?
        case Nothing:
            print("lost root")?
    Ok(())
"#,
        "Result[(), AllocError].Err(AllocError.OutOfMemory)\n1\nOption[i64].Some(1)\nOption[i64].Nothing\n1\n3\n2",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn arena_reuse_and_pair_updates_need_no_allocation_and_failed_growth_preserves_owners() {
    runs(
        r#"
from arena import Arena
class Item:
    value: i64
    def __del__(self) -> ():
        # Only the original payloads report cleanup; filler count depends on capacity.
        if self.value != 99:
            print(self.value).unwrap()
def increment(left: &mut Item, right: &mut Item) -> Result[bool, OverflowError[i64]]:
    left.value += 1
    right.value += 1
    Ok(True)
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut arena = Arena[Item](1)?
    first = arena.insert(Item(10))?
    second = arena.insert(Item(20))?
    drop(arena.remove(&first))
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    replacement = arena.insert(Item(30))?
    second_key = second.duplicate()
    result: Option[bool] = match arena.update_two(&replacement, &second_key, increment):
        case Some(result):
            Some(result?)
        case Nothing:
            Nothing
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(result)?
    print(arena.contains(&first))?
    # Fill any spare list capacity until the first attempted growth fails.
    print("__test_fail_allocations_after_0__")?
    mut failed = False
    for _ in range(100):
        match arena.insert(Item(99)):
            case Ok(_):
                pass
            case Err(_):
                failed = True
                break
    print("__test_restore_allocations__")?
    print(failed)?
    print(arena.contains(&replacement))?
    print(arena.contains(&second))?
    drop(arena.remove(&replacement))
    drop(arena.remove(&second))
    drop(arena)
    Ok(())
"#,
        "10\nOption[bool].Some(True)\nFalse\nTrue\nTrue\nTrue\n31\n21",
    );
}

#[cfg(all(feature = "runtime-checks", target_os = "linux"))]
#[test]
fn million_node_borrowed_walk_mutation_and_drop_use_a_small_stack_without_allocating() {
    let (_dir, executable) = compile(
        r#"
from linked import Linked, push, total, bump
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut chain = Linked(Nothing)
    for n in range(1, 1000001):
        push(&mut chain, n)?
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    before = total(&chain)?
    bump(&mut chain)?
    after = total(&chain)?
    drop(chain)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(before)?
    print(after)?
    Ok(())
"#,
    );
    let output = std::process::Command::new("sh")
        .args(["-c", "ulimit -s 256; exec \"$1\"", "data-structure-test"])
        .arg(executable)
        .output()
        .unwrap();
    assert_output(output, "500000500000\n500001500000");
}
