//! Explicit owned receivers reuse ordinary argument ownership and cleanup.
mod support;
use support::{assert_stdout as runs, check_error};

const STATE: &str = r#"
class State:
    id: i64
    def advance(self: State) -> State:
        self
    def finish(self: State, extra: i64 = 0) -> i64:
        self.id + extra
    def give[T](self: State, value: T) -> T:
        value
    def combine(self: State, a: i64 = 1, b: i64 = 2) -> i64:
        self.id + a + b
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
"#;

#[test]
fn owned_receivers_transfer_cleanup_for_locals_temporaries_and_chains() {
    runs(
        &format!(
            r#"{STATE}
first = State(1)
second = first.advance()
print(second.finish(extra=10)).unwrap()
print(State(2).advance().finish()).unwrap()
print(State(3).give("generic")).unwrap()
print(State(4).give[i64](value=42)).unwrap()
def argument(n: i64) -> i64:
    print("arg", n).unwrap()
    n
print(State(5).combine(b=argument(4), a=argument(3))).unwrap()
mut looped = State(6)
for i in range(2):
    looped = looped.advance()
drop(looped)
"#
        ),
        "drop 1\n11\ndrop 2\n2\ndrop 3\ngeneric\ndrop 4\n42\narg 4\narg 3\ndrop 5\n12\ndrop 6\n",
    );
}

#[test]
fn owned_generic_classes_and_enums_support_protocol_dispatch() {
    runs(
        r#"
protocol Finish[T]:
    def finish(self: Finish[T]) -> T:
        pass
class Cell[T]:
    value: T
    def finish(self: Cell) -> T:
        self.value
    def give[U](self: Cell[T], value: U) -> U:
        value
enum Packet[T]:
    Value(T)
    def finish(self: Packet) -> T:
        match self:
            case Packet.Value(value):
                value
def finish[T, F: Finish[T]](value: F) -> T:
    value.finish()
print(finish[i64, Cell[i64]](Cell[i64](7))).unwrap()
print(finish[list[i64], Packet[list[i64]]](Packet[list[i64]].Value([8].unwrap()))).unwrap()
print(Cell[i64](9).give[str]("explicit")).unwrap()
print(Cell[i64](10).give(value="inferred")).unwrap()
"#,
        "7\n[8]\nexplicit\ninferred\n",
    );
}

#[test]
fn value_receivers_on_copyable_enums_keep_ordinary_copy_semantics() {
    runs(
        r#"
enum Count:
    Value(i64)
    def take(self: Count) -> i64:
        match self:
            case Count.Value(n):
                n
value = Count.Value(3)
print(value.take(), value.take(), (&value).take()).unwrap()
"#,
        "3 3 3\n",
    );
}

#[test]
fn failures_clean_pending_owned_receivers_and_returned_retry_owners() {
    runs(
        &format!(
            r#"{STATE}
def fail() -> Result[i64, i64]:
    Err(7)
def later_argument() -> Result[(), i64]:
    drop(State(1).finish(extra=fail()?))
    Ok(())
def later_generic_argument() -> Result[(), i64]:
    drop(State(2).give(fail()?))
    Ok(())
class Draft:
    state: Option[State]
    def prepare(self: Draft, valid: bool) -> Result[State, Draft]:
        if not valid:
            return Err(self)
        mut owned = self
        Ok(replace(owned.state, Nothing).unwrap())
drop(later_argument())
drop(later_generic_argument())
draft = Draft(Some(State(3)))
match draft.prepare(False):
    case Ok(state):
        drop(state)
    case Err(retry):
        print(retry.prepare(True).unwrap().finish()).unwrap()
"#
        ),
        "drop 1\ndrop 2\ndrop 3\n3\n",
    );
}

#[test]
fn owned_receiver_can_move_into_mutable_local_and_generator_frame() {
    runs(
        r#"
class State:
    id: i64
    def bump(self: State) -> State:
        mut owned = self
        owned.id += 1
        owned
    def values(self: State) -> Generator[i64]:
        yield self.id
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
state = State(1).bump()
for value in state.values():
    print(value).unwrap()
unused = State(3).values()
drop(unused)
"#,
        "2\ndrop 2\ndrop 3\n",
    );
}

#[test]
fn consumed_receivers_cannot_be_reused_or_moved_through_live_loans() {
    for (body, expected) in [
        (
            "s = State(1)\ndrop(s.finish())\ndrop(s)",
            "use of moved binding `s`",
        ),
        (
            "s = State(1)\ndrop(s.finish(s.id))",
            "use of moved binding `s`",
        ),
        (
            "s = State(1)\nr = &s\ndrop(s.finish())\nprint(r.id).unwrap()",
            "borrow",
        ),
        (
            "s = State(1)\nif True:\n    drop(s.finish())\ndrop(s)",
            "moved",
        ),
        (
            "s = State(1)\nfor i in range(2):\n    drop(s.finish())",
            "moved",
        ),
        ("s = State(1)\nr = &s\ndrop(r.finish())", "expected State"),
        (
            "s = State(1)\ndrop((&s).finish())",
            "cannot consume a borrowed",
        ),
        (
            "s = State(1)\ndrop((&s).give(2))",
            "cannot consume a borrowed",
        ),
        (
            "def view(s: &State) -> &State:\n    s\ns = State(1)\ndrop(view(&s).finish())",
            "borrow",
        ),
        (
            "items = [State(1)].unwrap()\ndrop(items[0].finish())",
            "cannot move out of indexed storage",
        ),
        // A temporary's element is borrowed in place, as a named one is
        // (issue #599).
        (
            "drop([State(1)].unwrap()[0].finish())",
            "cannot move out of indexed storage",
        ),
    ] {
        let source = format!("{STATE}\n{body}");
        let error = check_error(&source);
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

#[test]
fn method_and_free_function_have_identical_optimized_machine_code() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(
        &path,
        r#"
class State:
    id: i64
    def finish(self: State) -> i64:
        self.id
def finish(state: State) -> i64:
    state.id
def method_call(state: State) -> i64:
    state.finish()
def function_call(state: State) -> i64:
    finish(state)
def main() -> Result[(), IoError]:
    print(method_call(State(1)), function_call(State(2)))?
    Ok(())
"#,
    )
    .unwrap();
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args(["--release", "--dump", "asm", "main.plenty", "-o", "out"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let assembly = std::fs::read_to_string(dir.path().join("out/asm.txt")).unwrap();
    let section = |name: &str| -> Vec<&str> {
        assembly
            .split("==== ")
            .find(|part| part.starts_with(&format!("{name} ")))
            .unwrap_or_else(|| panic!("missing assembly for {name}"))
            .lines()
            .skip(1)
            // Direct calls carry the callee's symbol as a disassembly comment.
            // Compare the instruction stream and each callee body separately.
            .map(|line| line.split('#').next().unwrap().trim_end())
            .filter(|line| !line.trim().is_empty())
            .collect()
    };
    assert_eq!(section("method_call"), section("function_call"));
    assert_eq!(section("State.finish"), section("finish"));
}

#[test]
fn owned_receivers_move_fields_out() {
    runs(
        &format!(
            r#"{STATE}
class Holder:
    state: State
    def take(self: Holder) -> State:
        self.state
h = Holder(State(1))
drop(h.take())
g = Holder(State(2))
print(g.state.finish()).unwrap()
print("end").unwrap()
"#
        ),
        // `finish` consumes the moved field, so it drops before its result prints.
        "drop 1\ndrop 2\n2\nend\n",
    );
}

#[test]
fn consuming_methods_do_not_change_special_hooks() {
    for (source, expected) in [
        (
            "class Bad:\n    def __del__(self: Bad) -> ():\n        pass",
            "lifecycle methods require",
        ),
        (
            "class Bad:\n    def __init__(self: Bad) -> ():\n        pass",
            "lifecycle methods require",
        ),
        (
            "class Bad:\n    def __neg__(self: Bad) -> Bad:\n        self",
            "requires shared self",
        ),
        (
            "class Bad:\n    id: i64\n    def borrow(self: Bad) -> &i64:\n        &self.id",
            "reference",
        ),
        (
            "class Bad:\n    id: i64\n    def borrow(self: Bad) -> Option[&i64]:\n        Some(&self.id)",
            "reference",
        ),
        (
            "class Bad:\n    id: i64\n    def borrow(self: Bad, other: &i64) -> Option[&i64]:\n        Some(&self.id)",
            "reference",
        ),
    ] {
        let error = check_error(&format!("{STATE}\n{source}"));
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

#[test]
fn protocols_distinguish_owned_from_borrowed_receivers() {
    for (required, actual) in [("Finish", "&State"), ("&Finish", "State")] {
        let source = format!(
            r#"
protocol Finish:
    def finish(self: {required}) -> i64:
        pass
class State:
    def finish(self: {actual}) -> i64:
        1
def invoke[T: Finish](value: T) -> i64:
    value.finish()
print(invoke(State())).unwrap()
"#
        );
        let error = check_error(&source);
        assert!(error.contains("does not satisfy"), "{error}");
    }
}
