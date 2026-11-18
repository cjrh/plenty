use super::test_std_alloc::assert_stdout;
use super::test_std_alloc_set_algebra::POLICY;
use crate::test_support::check_error;

#[test]
fn player_names_borrows_dictionary_and_allocator_and_returns_only_owned_keys() {
    assert_stdout(
        r#"import std.alloc as alloc
class Player:
    score: i64
    def __del__(self) -> ():
        print(self.score).unwrap()

def player_names(players: &dict[str, Player], out: &alloc.Allocator) -> Result[set[str], AllocError]:
    out.to_set(players.keys())

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    out = alloc.system()
    mut players = {("play" + "er")?: Player(7)}?
    view = players.keys()
    names = out.to_set[str](view)?
    print(len(view), len(names))?
    with spawn(player_names, &players, &out).drop_job()? as task:
        result = task.join()?
        print(names == result)?
    players.clear()
    drop(players)
    drop(out)
    print("player" in names)?
    Ok(())
"#,
        "1 1\nTrue\n7\nTrue\n",
    );
}

#[test]
fn selected_failures_roll_back_and_preserve_finite_and_affine_sources() {
    assert_stdout(
        &format!(
            r#"{POLICY}
@copy
enum Flag:
    A
    B
class Key:
    id: i64
    name: str

def main() -> Result[(), AllocError | IoError]:
    flags = {{Flag.A: 3, Flag.B: 4}}?
    refused = budget("finite", 0, AllocError.CapacityOverflow)
    print(refused.to_set(flags.keys()).is_err(), flags[Flag.A])?
    finite = budget("mask", 1)
    bits = finite.to_set(flags.keys())?
    drop(finite)
    drop(flags)
    print(Flag.B in bits, len(bits))?
    drop(bits)
    data = {{Key(42, ("aff" + "ine")?): [1]?}}?
    view = data.keys()
    partial = budget("partial", 3, AllocError.CapacityOverflow)
    match partial.to_set(view):
        case Err(error):
            print(error)?
        case Ok(value):
            drop(value)
    drop(partial)
    out = budget("keys", 5)
    mut keys = out.to_set(view)?
    print(len(view), data[Key(42, "affine")])?
    drop(data)
    drop(out)
    print(Key(42, "affine") in keys, keys.reserve(100).is_err())?
    drop(keys)
    Ok(())
"#
        ),
        "refused finite\nTrue 3\nTrue 2\nmask 0 1\nrefused partial\nAllocError.CapacityOverflow\npartial 0 3\n1 [1]\nrefused keys\nTrue True\nkeys 0 5\nfinite 0 0\n",
    );
}

#[test]
fn rejects_reference_to_view_wrong_collection_or_key_and_reserved_intrinsic() {
    for (call, message) in [
        ("out.to_set(&view)", "&KeyView"),
        ("out.to_set[str](view)", "KeyView[str]"),
        ("out.to_set[f64](view)", "does not satisfy KeyType"),
        ("out.to_set(data)", "dict[i64, i64]"),
        ("out.to_set(data.values())", "does not match"),
        (
            "__allocator_key_view_to_set[set[i64]](0, view)",
            "reserved for the standard library",
        ),
    ] {
        let error = check_error(&format!(
            "import std.alloc as alloc\ndef main() -> ():\n    out = alloc.system()\n    data = {{1: 1}}.unwrap()\n    view = data.keys()\n    drop({call})\n"
        ));
        assert!(error.contains(message), "{call}: {error}");
    }
}

#[test]
fn nested_destructor_in_key_is_not_duplicable_even_in_generic_code() {
    let error = check_error(
        r#"import std.alloc as alloc
class Leaf:
    id: i64
    def __del__(self) -> ():
        pass
class Key:
    leaf: Leaf

def convert[K: KeyType](data: &dict[K, bool], out: &alloc.Allocator) -> Result[set[K], AllocError]:
    out.to_set(data.keys())

def main() -> ():
    data = dict[Key, bool]().unwrap()
    drop(convert(&data, alloc.system()))
"#,
    );
    assert!(
        error.contains("requires copying keys") && error.contains("custom cleanup"),
        "{error}"
    );
}

#[test]
fn lookalike_allocator_method_is_not_privileged() {
    let error = check_error(
        r#"class Allocator:
    pub def to_set[K: KeyType](self, view: KeyView[K]) -> Result[set[K], AllocError]:
        view.to_set()

def main() -> ():
    data = {1: 2}.unwrap()
    drop(Allocator().to_set(data.keys()))
"#,
    );
    assert!(error.contains("local-only"), "{error}");
}

#[test]
fn view_loan_tracks_dictionary_not_allocator() {
    let error = check_error(
        r#"import std.alloc as alloc
def main() -> ():
    out = alloc.system()
    mut data = {1: 1}.unwrap()
    view = data.keys()
    data.clear()
    drop(out.to_set(view))
"#,
    );
    assert!(
        error.contains("borrow") && error.contains("data"),
        "{error}"
    );
}

#[test]
fn scoped_conversion_worker_holds_both_borrows() {
    for (action, diagnostic) in [
        ("drop(out)", "cannot move `out`"),
        ("data.clear()", "cannot modify or exclusively borrow `data`"),
    ] {
        let error = check_error(&format!(
            r#"import std.alloc as alloc
def work(data: &dict[i64, i64], out: &alloc.Allocator) -> Result[set[i64], AllocError]:
    view = data.keys()
    out.to_set(view)

def main() -> ():
    out = alloc.system()
    mut data = {{1: 1}}.unwrap()
    with spawn(work, &data, &out).drop_job().unwrap() as task:
        {action}
        drop(task.join())
"#
        ));
        assert!(
            error.contains(diagnostic) && error.contains("held until the end of this `with` block"),
            "{action}: {error}"
        );
    }
}

#[test]
fn excludes_actors_in_unused_body() {
    let error = check_error(
        r#"import std.alloc as alloc
import std.actor as actors
def unused(data: &dict[str, i64], out: &alloc.Allocator) -> ():
    drop(out.to_set(data.keys()))
def main() -> ():
    drop(actors.heaps_live())
"#,
    );
    assert!(
        error.contains("retained allocator use cannot be combined"),
        "{error}"
    );
}

#[test]
fn method_is_not_a_job_that_can_take_a_key_view() {
    for (job, expected) in [
        ("out.to_set, view", "takes no additional arguments"),
        (
            "def [&out, view]() -> Result[set[i64], AllocError]: out.to_set(view)",
            "cannot be captured",
        ),
        (
            "def [&out, &view]() -> Result[set[i64], AllocError]: out.to_set(view)",
            "cannot be captured",
        ),
    ] {
        let error = check_error(&format!(
            r#"import std.alloc as alloc
def main() -> ():
    out = alloc.system()
    data = {{1: 2}}.unwrap()
    view = data.keys()
    with spawn({job}).unwrap() as task:
        drop(task.join())
"#
        ));
        assert!(error.contains(expected), "{job}: {error}");
    }
}

#[test]
fn worker_checks_the_dictionary_value_even_when_projecting_only_keys() {
    let error = check_error(
        r#"import std.alloc as alloc
import std.env
class Resource:
    id: i64
    def __del__(self) -> ():
        path = std.env.current_dir().unwrap()
        std.env.set_current_dir(&path).unwrap()

def work(out: &alloc.Allocator, data: &dict[i64, Resource]) -> Result[set[i64], AllocError]:
    out.to_set(data.keys())

def main() -> ():
    data = dict[i64, Resource]().unwrap()
    out = alloc.system()
    with spawn(work, &out, &data).drop_job().unwrap() as task:
        drop(task.join())
"#,
    );
    assert!(error.contains("changing the working directory"), "{error}");
}

#[test]
fn indirect_call_cannot_take_a_dictionary_key_projection() {
    let error = check_error(
        r#"def invoke(f: Callable[[KeyView[i64]], ()], data: &dict[i64, i64]) -> ():
    f(data.keys())
def main() -> ():
    pass
"#,
    );
    assert!(
        error.contains("dictionary views are local-only") && error.contains("callable"),
        "{error}"
    );
}

#[test]
fn callable_signatures_exclude_key_views_without_any_call() {
    for signature in [
        "Callable[[], KeyView[i64]]",
        "BoxedCallable[[&KeyView[i64]], ()]",
        "Callable[[&mut Iter[KeyView[i64]]], ()]",
        "Callable[[&mut Iter[Generator[&KeyView[i64]]]], ()]",
        "Callable[[Callable[[], KeyView[i64]]], ()]",
        "Callable[[], BoxedOnceCallable[[KeyView[i64]], ()]]",
    ] {
        let error = check_error(&format!(
            "def unused(f: {signature}) -> ():\n    pass\ndef main() -> ():\n    pass\n"
        ));
        assert!(
            error.contains("dictionary views are local-only") && error.contains("callable"),
            "{signature}: {error}"
        );
    }
}

#[test]
fn fixed_key_view_parameter_cannot_bypass_generic_call_argument_checks() {
    let error = check_error(
        r#"def ignore[T](view: KeyView[i64], value: T) -> ():
    pass
def main() -> ():
    data = {1: 2}.unwrap()
    ignore(data.keys(), 1)
"#,
    );
    assert!(
        error.contains("local-only") && error.contains("cannot be passed to this call"),
        "{error}"
    );
}
