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
def increment(x: &mut i64) -> i64:
    *x += 1
    *x
def add(left: &mut i64, right: &mut i64) -> bool:
    *left += *right
    True
def main() -> Result[(), AllocError | IoError]:
    mut arena = Arena[i64](10, generation_limit=1)?
    mut other = Arena[i64](11)?
    first = arena.insert(1)?
    foreign = other.insert(99)?
    print(arena.contains(&foreign))?
    print(arena.update_two(&first, &first, add))?
    print(arena.remove(&first))?
    second = arena.insert(2)?
    print(arena.contains(&first))?
    print(arena.remove(&first))?
    print(arena.remove(&second))?
    third = arena.insert(3)?
    print(arena.contains(&first))?
    print(arena.contains(&second))?
    print(arena.read(&third, value))?
    print(arena.update(&third, increment))?
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
def change(left: &mut Item, right: &mut Item) -> Result[bool, Stopped]:
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
        "Option[Result[bool, Stopped]].Some(Result[bool, Stopped].Err(Stopped.Done))\nTrue\nTrue\n11\n22",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_push_preserves_existing_chain_and_rotation_keeps_allocations() {
    runs(
        r#"
from linked import Linked, push, total, pop
from tree import Tree, rotate_right
def main() -> Result[(), AllocError | IoError]:
    mut chain = Linked(Nothing)
    push(&mut chain, 1)?
    print("__test_fail_allocations_after_0__")?
    failure = push(&mut chain, 2)
    print("__test_restore_allocations__")?
    print(str.repr(failure)?)?
    print(total(&chain))?
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
def increment(left: &mut Item, right: &mut Item) -> bool:
    left.value += 1
    right.value += 1
    True
def main() -> Result[(), AllocError | IoError]:
    mut arena = Arena[Item](1)?
    first = arena.insert(Item(10))?
    second = arena.insert(Item(20))?
    drop(arena.remove(&first))
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    replacement = arena.insert(Item(30))?
    second_key = second.duplicate()
    result = arena.update_two(&replacement, &second_key, increment)
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
def main() -> Result[(), AllocError | IoError]:
    mut chain = Linked(Nothing)
    for n in range(1, 1000001):
        push(&mut chain, n)?
    print("__test_begin_no_allocations__")?
    print("__test_fail_allocations_after_0__")?
    before = total(&chain)
    bump(&mut chain)
    after = total(&chain)
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
