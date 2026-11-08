//! Anonymous sums satisfy eligible shared-receiver protocols by dispatching
//! each requirement to the active member (issue #875).
use std::path::Path;
use std::process::Command;

fn build(directory: &Path, source: &str) -> std::process::Output {
    let file = directory.join("main.plenty");
    let binary = directory.join("program");
    std::fs::write(&file, source).unwrap();
    plenty::compile_file_to_executable(&file, &binary, None)
        .unwrap_or_else(|error| panic!("{error}"));
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn run(source: &str) -> String {
    let directory = tempfile::tempdir().unwrap();
    let output = build(directory.path(), source);
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rejects(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("main.plenty");
    std::fs::write(&file, source).unwrap();
    let error = plenty::check_file(&file, None).unwrap_err().to_string();
    assert!(error.contains(expected), "expected {expected:?}: {error}");
}

const LABELLED: &str = r#"
protocol Labelled:
    def label(self) -> str:
        pass
    def weight(self, scale: i64, calls: &mut i64) -> i64:
        pass
class A:
    pub def label(self) -> str:
        return "a"
    pub def weight(self, scale: i64, calls: &mut i64) -> i64:
        unchecked:
            *calls += 1
        scale
enum B:
    One
    Two(i64)
    pub def label(self) -> str:
        return "b"
    pub def weight(self, scale: i64, calls: &mut i64) -> i64:
        unchecked:
            *calls += 10
        match self:
            case B.One:
                0
            case B.Two(n):
                *n
def label[T: Labelled](value: &T) -> str:
    value.label()
def weigh[T: Labelled](value: &T, scale: i64, calls: &mut i64) -> i64:
    value.weight(scale, calls)
"#;

#[test]
fn every_requirement_dispatches_once_to_the_active_member() {
    assert_eq!(
        run(&format!(
            r#"{LABELLED}
type Either = B | A
def main() -> Result[(), IoError]:
    first: A | B = A()
    second: Either = B.Two(7)
    mut calls = 0
    print(label(&first), label(&second))?
    print(weigh(&first, 3, &mut calls), calls)?
    print(weigh(&second, 3, &mut calls), calls)?
    Ok(())
"#
        )),
        "a b\n3 1\n7 11"
    );
}

#[test]
fn aliases_and_member_order_share_one_instantiation() {
    // `A | B`, `B | A` and the alias normalize to one sum, so the bound and
    // the dispatcher are checked and synthesized once for all three spellings.
    assert_eq!(
        run(&format!(
            r#"{LABELLED}
type Either = B | A
def pick(flag: bool) -> A | B:
    if flag:
        return A()
    B.One
def main() -> Result[(), IoError]:
    left: A | B = pick(True)
    right: B | A = pick(False)
    alias: Either = pick(True)
    print(label(&left), label(&right), label(&alias))?
    Ok(())
"#
        )),
        "a b a"
    );
}

#[test]
fn inferred_type_parameters_and_method_generics_forward() {
    assert_eq!(
        run(r#"
protocol Scaled:
    def scaled[U](self, item: &U, factor: i64) -> i64:
        pass
class Small:
    pub def scaled[U](self, item: &U, factor: i64) -> i64:
        factor
class Large:
    pub def scaled[U](self, item: &U, factor: i64) -> i64:
        200
def apply[T: Scaled, U](value: &T, item: &U) -> i64:
    value.scaled(item, 2)
def main() -> Result[(), IoError]:
    small: Small | Large = Small()
    large: Small | Large = Large()
    print(apply(&small, &1), apply(&large, &True))?
    Ok(())
"#),
        "2 200"
    );
}

#[test]
fn recursive_method_bounds_accept_the_sum_itself() {
    assert_eq!(
        run(r#"
protocol Paired:
    def pair[U: Paired](self, other: &U) -> i64:
        pass
class A:
    pub def pair[U: Paired](self, other: &U) -> i64:
        1
class B:
    pub def pair[U: Paired](self, other: &U) -> i64:
        2
def use[T: Paired](value: &T) -> i64:
    value.pair(value)
def main() -> Result[(), IoError]:
    value: A | B = B()
    print(use(&value))?
    Ok(())
"#),
        "2"
    );
}

#[test]
fn recursive_method_bounds_report_the_failing_member() {
    rejects(
        r#"
protocol Paired:
    def pair[U: Paired](self, other: &U) -> i64:
        pass
class A:
    pub def pair[U: Paired](self, other: &U) -> i64:
        1
class C:
    pub def other(self) -> i64:
        3
def use[T: Paired](value: &T) -> i64:
    value.pair(value)
def main() -> ():
    value: A | C = A()
    use(&value)
"#,
        "C does not satisfy Paired: missing method `pair`",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn dispatch_does_not_allocate() {
    assert_eq!(
        run(&format!(
            r#"{LABELLED}
def main() -> Result[(), IoError]:
    value: A | B = B.Two(4)
    mut calls = 0
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    first = weigh(&value, 5, &mut calls)
    second = weigh(&value, 1, &mut calls)
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(first, second, calls)?
    Ok(())
"#
        )),
        "4 4 20"
    );
}

#[test]
fn missing_and_mismatched_members_are_rejected() {
    for (member, expected) in [
        (
            "pub def name(self) -> str:\n        return \"c\"",
            "C does not satisfy Labelled: missing method `label`",
        ),
        (
            "pub def label(self) -> i64:\n        1",
            "signature of `label` does not match",
        ),
    ] {
        rejects(
            &format!(
                r#"
protocol Labelled:
    def label(self) -> str:
        pass
class A:
    pub def label(self) -> str:
        return "a"
class C:
    {member}
def label[T: Labelled](value: &T) -> str:
    value.label()
def main() -> ():
    value: A | C = A()
    label(&value)
"#
            ),
            expected,
        );
    }
}

#[test]
fn direct_calls_do_not_forward_even_after_a_bound_does() {
    // Only a body that received the sum through a bound dispatches; a bound
    // checked earlier must not make a direct call elsewhere compile.
    rejects(
        &format!(
            r#"{LABELLED}
def main() -> Result[(), IoError]:
    value: A | B = A()
    print(label(&value))?
    print(value.label())?
    Ok(())
"#
        ),
        "unknown method `label` on",
    );
}

#[test]
fn lifted_code_in_a_bounded_body_dispatches() {
    assert_eq!(
        run(&format!(
            r#"{LABELLED}
def lifted[T: Labelled](value: &T) -> str:
    inner = def [&value]() -> str:
        value.label()
    inner()
def main() -> Result[(), IoError]:
    value: A | B = B.One
    print(lifted(&value))?
    Ok(())
"#
        )),
        "b"
    );
}

/// Writes the modules, then checks `main.plenty`.
fn check_modules(modules: &[(&str, &str)]) -> Result<(), String> {
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in modules {
        std::fs::write(directory.path().join(name), source).unwrap();
    }
    plenty::check_file(&directory.path().join("main.plenty"), None)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

const SHAPES: &str = r#"
pub protocol Secret:
    def secret(self) -> str:
        pass
pub class Square:
    def secret(self) -> str:
        return "square"
pub class Circle:
    def secret(self) -> str:
        return "circle"
"#;

#[test]
fn private_members_stay_private_through_a_foreign_protocol() {
    let direct = check_modules(&[
        ("shapes.plenty", SHAPES),
        (
            "main.plenty",
            "from shapes import Square, Circle\ndef main() -> ():\n    value: Square | Circle = Square()\n    value.secret()\n",
        ),
    ])
    .unwrap_err();
    assert!(direct.contains("unknown method `secret` on"), "{direct}");
    let bound = check_modules(&[
        ("shapes.plenty", SHAPES),
        (
            "main.plenty",
            "from shapes import Square, Circle, Secret\ndef reveal[T: Secret](value: &T) -> str:\n    value.secret()\ndef main() -> ():\n    value: Square | Circle = Square()\n    reveal(&value)\n",
        ),
    ])
    .unwrap_err();
    assert!(bound.contains("private to its defining module"), "{bound}");
}

#[test]
fn transitively_loaded_protocols_do_not_affect_direct_calls() {
    let helper = "protocol Named:\n    def name(self) -> str:\n        pass\npub def helper() -> i64:\n    1\n";
    let mid = "import helper\npub def mid() -> i64:\n    helper.helper()\n";
    let main = "import mid\npub class A:\n    pub def name(self) -> str:\n        return \"a\"\npub class B:\n    pub def name(self) -> str:\n        return \"b\"\ndef main() -> ():\n    value: A | B = A()\n    value.name()\n";
    let error = check_modules(&[
        ("helper.plenty", helper),
        ("mid.plenty", mid),
        ("main.plenty", main),
    ])
    .unwrap_err();
    assert!(error.contains("unknown method `name` on"), "{error}");
}

#[test]
fn members_private_to_another_module_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("shapes.plenty"),
        r#"
pub class Square:
    def label(self) -> str:
        return "square"
pub class Circle:
    pub def label(self) -> str:
        return "circle"
"#,
    )
    .unwrap();
    let main = directory.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
from shapes import Square, Circle
protocol Labelled:
    def label(self) -> str:
        pass
def label[T: Labelled](value: &T) -> str:
    value.label()
def main() -> ():
    value: Square | Circle = Circle()
    label(&value)
"#,
    )
    .unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(error.contains("private to its defining module"), "{error}");
}

#[test]
fn ineligible_requirements_name_the_restriction() {
    for (requirement, member_a, member_b, call, expected) in [
        (
            "def f(self: &mut P) -> i64:",
            "pub def f(self: &mut A) -> i64:\n        1",
            "pub def f(self: &mut B) -> i64:\n        2",
            "mut value: A | B = A()\n    use(&mut value)",
            "`f` has a mutable receiver",
        ),
        (
            "def f(self: P) -> i64:",
            "pub def f(self: A) -> i64:\n        1",
            "pub def f(self: B) -> i64:\n        2",
            "value: A | B = A()\n    use(value)",
            "`f` has a consuming receiver",
        ),
        (
            "def f(self) -> P:",
            "pub def f(self) -> A:\n        A()",
            "pub def f(self) -> B:\n        B()",
            "value: A | B = A()\n    use(&value)",
            "`f` uses the implementing type outside its receiver",
        ),
        (
            "def f(self, other: &P) -> i64:",
            "pub def f(self, other: &A) -> i64:\n        1",
            "pub def f(self, other: &B) -> i64:\n        2",
            "value: A | B = A()\n    use(&value)",
            "`f` uses the implementing type outside its receiver",
        ),
        (
            "def f(self) -> &i64:",
            "pub def f(self) -> &i64:\n        1",
            "pub def f(self) -> &i64:\n        2",
            "value: A | B = A()\n    use(&value)",
            "`f` returns a reference or view",
        ),
        (
            "def f[U](self) -> i64:",
            "pub def f[U](self) -> i64:\n        1",
            "pub def f[U](self) -> i64:\n        2",
            "value: A | B = A()\n    use(&value)",
            "`f` has a type parameter `U` that no argument determines",
        ),
    ] {
        let parameter = if call.contains("use(&mut") {
            "&mut T"
        } else if call.contains("use(value)") {
            "T"
        } else {
            "&T"
        };
        rejects(
            &format!(
                r#"
protocol P:
    {requirement}
        pass
class A:
    {member_a}
class B:
    {member_b}
def use[T: P](value: {parameter}) -> ():
    pass
def main() -> ():
    {call}
"#
            ),
            &format!(
                "A | B does not satisfy P: an anonymous sum cannot forward it because {expected}"
            ),
        );
    }
}

#[test]
fn generic_protocols_do_not_forward() {
    rejects(
        r#"
protocol Holder[V]:
    def get(self) -> V:
        pass
class A:
    pub def get(self) -> i64:
        1
class B:
    pub def get(self) -> i64:
        2
def use[T: Holder[i64]](value: &T) -> ():
    pass
def main() -> ():
    value: A | B = A()
    use(&value)
"#,
        "an anonymous sum cannot forward it because it has type parameters",
    );
}
