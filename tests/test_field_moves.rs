//! Moving a field out of an owned binding, tracked per field (issue #534).
//! A `__del__` hook prints each resource as it is destroyed, so the runtime
//! tests show which fields are destroyed and that each is destroyed once. With
//! `--all-features`, the allocation-checks runtime also checks at exit that
//! every list and string field was released.
mod support;
use support::{assert_stdout as runs, check_error};

const RESOURCES: &str = r#"
class Res:
    name: str
    def __del__(self) -> ():
        print("drop", self.name).unwrap()
class Pair:
    a: Res
    b: Res
    n: i64
class Outer:
    inner: Pair
    c: Res
def take(r: Res) -> ():
    print("take", r.name).unwrap()
"#;

fn rejects(body: &str, expected: &str) {
    let error = check_error(&format!("{RESOURCES}\n{body}"));
    assert!(error.contains(expected), "{body}\n{error}");
}

#[test]
fn a_field_moves_out_and_the_other_fields_stay_usable() {
    runs(
        &format!(
            r#"{RESOURCES}
p = Pair(Res("a"), Res("b"), 3)
x = p.a
print(p.n).unwrap()
print(p.b.name).unwrap()
take(x)
y = p.b
print("end").unwrap()
"#
        ),
        "3\nb\ntake a\ndrop a\nend\ndrop b\n",
    );
}

#[test]
fn only_initialized_fields_drop_at_scope_end() {
    runs(
        &format!(
            r#"{RESOURCES}
def f() -> ():
    p = Pair(Res("a"), Res("b"), 3)
    take(p.a)
    print("leaving").unwrap()
f()
print("end").unwrap()
"#
        ),
        "take a\ndrop a\nleaving\ndrop b\nend\n",
    );
}

#[test]
fn owned_receivers_move_fields_into_the_next_state() {
    runs(
        &format!(
            r#"{RESOURCES}
class Pending:
    items: list[str]
    tag: Res
    def ship(self: Pending) -> list[str]:
        self.items
def main() -> Result[(), AllocError | IoError]:
    order = Pending(["book", "pen"]?, Res("tag"))
    items = order.ship()
    print(len(items))?
    Ok(())
"#
        ),
        "drop tag\n2\n",
    );
}

#[test]
fn assignment_restores_the_whole_object() {
    runs(
        &format!(
            r#"{RESOURCES}
mut p = Pair(Res("a"), Res("b"), 3)
take(p.a)
p.a = Res("c")
whole = p
print("end").unwrap()
"#
        ),
        "take a\ndrop a\nend\ndrop c\ndrop b\n",
    );
    runs(
        &format!(
            r#"{RESOURCES}
mut p = Pair(Res("a"), Res("b"), 3)
take(p.a)
p = Pair(Res("c"), Res("d"), 4)
print("end").unwrap()
"#
        ),
        "take a\ndrop a\ndrop b\nend\ndrop c\ndrop d\n",
    );
}

#[test]
fn nested_fields_move_and_are_restored_by_assigning_a_parent() {
    runs(
        &format!(
            r#"{RESOURCES}
mut o = Outer(Pair(Res("a"), Res("b"), 1), Res("c"))
take(o.inner.a)
print(o.inner.b.name).unwrap()
o.inner = Pair(Res("x"), Res("y"), 2)
whole = o
print("end").unwrap()
"#
        ),
        "take a\ndrop a\nb\ndrop b\nend\ndrop x\ndrop y\ndrop c\n",
    );
    rejects(
        "o = Outer(Pair(Res('a'), Res('b'), 1), Res('c'))\ntake(o.inner.a)\ninner = o.inner",
        "use of partly moved field `o.inner`: `o.inner.a` is moved",
    );
    rejects(
        "mut o = Outer(Pair(Res('a'), Res('b'), 1), Res('c'))\ninner = o.inner\no.inner.a = Res('x')",
        "use of moved field `o.inner`",
    );
}

#[test]
fn conditional_moves_drop_the_field_only_where_it_remains() {
    let source = format!(
        r#"{RESOURCES}
def f(flag: bool) -> ():
    p = Pair(Res("a"), Res("b"), 3)
    if flag:
        take(p.a)
    print("joined").unwrap()
f(True)
f(False)
"#
    );
    runs(
        &source,
        "take a\ndrop a\njoined\ndrop b\njoined\ndrop a\ndrop b\n",
    );
    rejects(
        "p = Pair(Res('a'), Res('b'), 3)\nif p.n > 2:\n    take(p.a)\ntake(p.a)",
        "use of possibly moved field `p.a`",
    );
    rejects(
        "p = Pair(Res('a'), Res('b'), 3)\nif p.n > 2:\n    take(p.a)\nwhole = p",
        "use of partly moved binding `p`: `p.a` is moved on some paths",
    );
}

#[test]
fn early_returns_drop_the_remaining_fields() {
    runs(
        &format!(
            r#"{RESOURCES}
def f(flag: bool) -> ():
    p = Pair(Res("a"), Res("b"), 3)
    take(p.a)
    if flag:
        return
    print("continued").unwrap()
f(True)
f(False)
"#
        ),
        "take a\ndrop a\ndrop b\ntake a\ndrop a\ncontinued\ndrop b\n",
    );
}

#[test]
fn loops_must_assign_a_moved_field_before_the_next_iteration() {
    runs(
        &format!(
            r#"{RESOURCES}
mut p = Pair(Res("a"), Res("b"), 3)
for i in range(2):
    take(p.a)
    p.a = Res("again")
print("end").unwrap()
"#
        ),
        "take a\ndrop a\ntake again\ndrop again\nend\ndrop again\ndrop b\n",
    );
    rejects(
        "p = Pair(Res('a'), Res('b'), 3)\nfor i in range(2):\n    take(p.a)",
        "`p.a` is moved in a loop",
    );
}

#[test]
fn whole_uses_of_a_partly_moved_binding_are_rejected() {
    for whole in [
        "q = p",
        "drop(p)",
        "r = &p",
        "print(p.a.name).unwrap()",
        "y = p.a",
        "z = replace(p.a, Res('z'))",
    ] {
        let expected = if whole.contains("p.a") {
            "use of moved field `p.a`"
        } else {
            "use of partly moved binding `p`: `p.a` is moved; assign `p.a` before using `p` as a whole"
        };
        rejects(
            &format!("mut p = Pair(Res('a'), Res('b'), 3)\nx = p.a\n{whole}"),
            expected,
        );
    }
    rejects(
        "class Holder:\n    r: Res\n    def show(self) -> ():\n        pass\nh = Holder(Res('a'))\nx = h.r\nh.show()",
        "use of partly moved binding `h`",
    );
    rejects(
        "def back(p: Pair) -> Pair:\n    x = p.a\n    p",
        "use of partly moved binding `p`",
    );
}

#[test]
fn a_borrowed_field_cannot_move_out_but_another_can() {
    rejects(
        "p = Pair(Res('a'), Res('b'), 3)\nr = &p.a\ntake(p.a)\nprint(r.name).unwrap()",
        "while it is borrowed",
    );
    runs(
        &format!(
            r#"{RESOURCES}
p = Pair(Res("a"), Res("b"), 3)
r = &p.b
take(p.a)
print(r.name).unwrap()
"#
        ),
        "take a\ndrop a\nb\ndrop b\n",
    );
}

#[test]
fn references_and_other_owners_keep_fields_in_place() {
    rejects(
        "def steal(p: &mut Pair) -> Res:\n    p.a",
        "cannot move out of a field through a reference; use &value.field (or &mut value.field), or replace(value.field, replacement) through a mutable reference",
    );
    rejects(
        "class Holder:\n    r: Res\n    def steal(self) -> Res:\n        self.r",
        "cannot move out of a field through a reference",
    );
    rejects(
        "def main() -> Result[(), AllocError]:\n    p = Box(Pair(Res('a'), Res('b'), 3))?\n    take(p.a)\n    Ok(())",
        "cannot move out of a field here: fields move out only from a named class binding",
    );
    rejects(
        "take(Pair(Res('a'), Res('b'), 3).a)",
        "cannot move out of a field here",
    );
}

#[test]
fn classes_with_destructors_keep_every_field() {
    rejects(
        "class Holder:\n    r: Res\n    def __del__(self) -> ():\n        pass\nh = Holder(Res('a'))\ntake(h.r)",
        "cannot move out of a field of `Holder`: its `__del__` expects every field initialized; use replace(value.field, replacement) on a mutable owner",
    );
    rejects(
        "class Holder:\n    p: Pair\n    def __del__(self) -> ():\n        pass\nh = Holder(Pair(Res('a'), Res('b'), 1))\ntake(h.p.a)",
        "cannot move out of a field of `Holder`",
    );
    // A field whose own class has `__del__` moves whole, hook included.
    runs(
        &format!(
            r#"{RESOURCES}
class Holder:
    r: Res
    s: Res
h = Holder(Res("r"), Res("s"))
take(h.r)
print("end").unwrap()
"#
        ),
        "take r\ndrop r\nend\ndrop s\n",
    );
}

#[test]
fn generic_owned_fields_move_out() {
    runs(
        &format!(
            r#"{RESOURCES}
class Cell[T]:
    value: T
    spare: T
    def into_value(self: Cell[T]) -> T:
        self.value
take(Cell[Res](Res("v"), Res("w")).into_value())
print("end").unwrap()
"#
        ),
        "drop w\ntake v\ndrop v\nend\n",
    );
}
