mod support;

fn independent_templates(count: usize) -> String {
    use std::fmt::Write;
    let mut source = String::new();
    for i in 0..count {
        writeln!(source, "def generic_case_{i}[T](value: T) -> T:\n    value").unwrap();
    }
    source.push_str("def main() -> ():\n");
    for i in 0..count {
        writeln!(source, "    drop(generic_case_{i}(0))").unwrap();
    }
    source
}

#[test]
fn unrelated_templates_do_not_share_the_expansion_limit() {
    plenty::check_source(&independent_templates(512)).unwrap();
}

#[test]
fn a_program_can_have_more_instances_than_one_template() {
    // The total budget formerly equalled the per-template limit (issue #428).
    plenty::check_source(&independent_templates(16_385)).unwrap();
}

#[test]
#[ignore = "creates 262,144 instances: about 1.4 GB and 11 s in a debug build; run with --ignored"]
fn total_specialization_budget_has_a_resource_diagnostic() {
    // Seventeen runaways grow together, so the total runs out before any one
    // template reaches its own limit.
    use std::fmt::Write;
    let mut source = String::new();
    for i in 0..17 {
        writeln!(
            source,
            "def f{i}[T]() -> ():\n    f{i}[list[T]]()\n    f{i}[Box[T]]()"
        )
        .unwrap();
    }
    source.push_str("def main() -> ():\n");
    for i in 0..17 {
        writeln!(source, "    f{i}[i64]()").unwrap();
    }
    let error = rejection(&source);
    assert!(
        error.contains("total generic function specialization budget of 262144 exceeded"),
        "{error}"
    );
}

#[test]
fn branching_expansion_reaches_the_per_template_limit_within_the_depth_limit() {
    let error = rejection(
        "def f[T]() -> ():\n    f[list[T]]()\n    f[Box[T]]()\ndef main() -> ():\n    f[i64]()\n",
    );
    assert!(
        error.contains("generic specialization limit of 16384 instances of `f` exceeded"),
        "{error}"
    );
}

#[test]
fn one_template_can_have_more_than_256_independent_instances() {
    use std::fmt::Write;
    let mut source = String::from("def identity[T](value: T) -> T:\n    value\n");
    for i in 0..257 {
        writeln!(source, "class Tag{i}:\n    value: i64").unwrap();
    }
    source.push_str("def main() -> ():\n");
    for i in 0..256 {
        writeln!(source, "    drop(identity(Tag{i}(0)))").unwrap();
    }
    // Explicit and inferred uses still share an existing instance.
    source.push_str("    drop(identity[Tag0](Tag0(0)))\n    drop(identity(Tag0(1)))\n");
    plenty::check_source(&source).unwrap();
    source.push_str("    drop(identity(Tag256(0)))\n");
    plenty::check_source(&source).unwrap();
}

#[test]
fn dependency_depth_is_bounded_separately_from_breadth() {
    use std::fmt::Write;
    let mut source = String::new();
    for i in 0..256 {
        writeln!(
            source,
            "def step_{i}[T](value: T) -> T:\n    step_{}(value)",
            i + 1
        )
        .unwrap();
    }
    source.push_str(
        "def step_256[T](value: T) -> T:\n    value\ndef main() -> ():\n    drop(step_1(0))\n",
    );
    plenty::check_source(&source).unwrap();
    // A cycle back to a cached instance does not add a dependency level.
    plenty::check_source(&source.replace(
        "def step_256[T](value: T) -> T:\n    value",
        "def step_256[T](value: T) -> T:\n    step_1(value)",
    ))
    .unwrap();
    let source = source.replace("drop(step_1(0))", "drop(step_0(0))");
    let error = rejection(&source);
    assert!(
        error.contains("dependency depth limit of 256 exceeded"),
        "{error}"
    );
}

#[test]
fn expanding_closure_identities_keep_the_depth_guard_through_lifted_code() {
    for body in [
        "    grow(def once (n: i64) -> i64: n)\n",
        "    next = def () -> ():\n        grow(def once (n: i64) -> i64: n)\n    next()\n",
    ] {
        let source = format!(
            "def grow[T](value: T) -> ():\n    drop(value)\n{body}\ndef main() -> ():\n    grow(0)\n"
        );
        let error = rejection(&source);
        assert!(
            error.contains("dependency depth limit of 256 exceeded"),
            "{error}"
        );
    }
}

#[test]
fn explicit_functions_specialize_numeric_owned_and_borrowed_values() {
    let out = support::run(
        r#"
def identity[T](value: T) -> T:
    value
def add[T: IntType](a: T, b: T) -> T:
    a + b
def total[T: IntType](stop: T) -> T:
    mut value: T = 0
    for n in range[T](stop):
        value = value + n
    value
def first[T](values: &list[T]) -> &T:
    &values[0]
def down[T: IntType](n: T) -> T:
    if n == 0:
        return n
    down[T](n - 1)
print(identity[i64](3)).unwrap()
print(identity[str]("hello")).unwrap()
print(add[u8](2, 3)).unwrap()
print(total[u16](5)).unwrap()
items = identity[list[i64]]([1, 2].unwrap())
print(first[i64](&items)).unwrap()
print(items).unwrap()
print(down[i32](1000)).unwrap()
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "3\nhello\n5\n10\n1\n[1, 2]\n0\n"
    );
}

#[test]
fn invalid_type_arguments_and_concrete_bodies_are_diagnosed() {
    for source in [
        "def f[T]() -> T:\n    1\nf()",
        "def f[T](x: T) -> T:\n    x\nf[i64, i32](1)",
        "def f[T: IntType](x: T) -> T:\n    x\nf[str]('bad')",
        "def f[T](x: T) -> T:\n    x + True\nf[i64](1)",
        "def f[T](x: T) -> T:\n    x\nf[u8](256)",
        "def f[T](x: T) -> T:\n    x\na = [1].unwrap()\nb = f[list[i64]](a)\nprint(a).unwrap()",
        "def f[T, T](x: T) -> T:\n    x",
        "def f[T](T: i64) -> i64:\n    T\nf[str](1)",
        "def f[T](x: T) -> T:\n    x\nf = 1\nf[i64](2)",
        "def f[T](x: T) -> ():\n    f[list[T]]([x].unwrap())\nf[i64](1)",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

fn rejection(source: &str) -> String {
    plenty::check_source(source).unwrap_err().to_string()
}

#[test]
fn instance_body_errors_are_reported_at_the_first_requiring_use() {
    // Found by the ownership pass: the body note precedes the pass's own notes.
    assert_eq!(
        rejection(
            "def twice[T](value: T) -> tuple[T, T]:
    (value, value)

def helper() -> Result[(), AllocError]:
    a = twice(1)
    b = twice([1]?)
    Ok(())

def main() -> Result[(), AllocError]:
    c = twice([2]?)
    helper()
"
        ),
        "6:9: cannot instantiate `twice[list[i64]]`: its definition is rejected
  2:13: note: in instantiation `twice[list[i64]]`: use of moved binding `value`
  2:6: note: `value` is moved here"
    );
    // Found while lowering the body.
    assert_eq!(
        rejection(
            "def sum[T](a: T, b: T) -> T:
    a + b

def main() -> ():
    x = sum[u8](1, 2)
    y = sum[bool](True, False)
"
        ),
        "6:9: cannot instantiate `sum[bool]`: its definition is rejected
  2:5: note: in instantiation `sum[bool]`: operator `+` does not accept bool"
    );
    // Found by tail-call validation, after every body is lowered.
    assert_eq!(
        rejection(
            "def walk[T](value: T, seen: &i64, n: i64) -> i64:
    if n == 0:
        return 0
    local = n
    walk(value, &local, n - 1)

def main() -> ():
    zero = 0
    x = walk(1u8, &zero, 3)
"
        ),
        "9:9: cannot instantiate `walk[u8]`: its definition is rejected
  5:5: note: in instantiation `walk[u8]`: recursive call to `walk[u8]` is in tail position \
         but cannot be a tail call: the reference argument borrows local `local`, whose storage \
         does not survive the jump (borrow at 5:18)"
    );
}

#[test]
fn nested_instance_errors_name_the_outermost_use_and_the_failing_instance() {
    assert_eq!(
        rejection(
            "def twice[T](value: T) -> tuple[T, T]:
    (value, value)

def middle[T](value: T) -> tuple[T, T]:
    twice(value)

def outer[T](value: T) -> tuple[T, T]:
    middle(value)

def main() -> Result[(), AllocError]:
    a = outer(1)
    b = outer([1]?)
    Ok(())
"
        ),
        "12:9: cannot instantiate `outer[list[i64]]`: its definition is rejected
  2:13: note: in instantiation `twice[list[i64]]`: use of moved binding `value`
  2:6: note: `value` is moved here"
    );
}

#[test]
fn generic_method_and_lifted_function_errors_are_reported_at_the_use() {
    assert_eq!(
        rejection(
            "class Holder:
    n: i64
    def both[T](self, value: T) -> tuple[T, T]:
        (value, value)

def main() -> Result[(), AllocError]:
    holder = Holder(1)
    a = holder.both(1)
    b = holder.both([1]?)
    Ok(())
"
        ),
        "9:9: cannot instantiate `Holder.both[list[i64]]`: its definition is rejected
  4:17: note: in instantiation `Holder.both[list[i64]]`: use of moved binding `value`
  4:10: note: `value` is moved here"
    );
    // An anonymous function is lowered apart from the instance that contains it.
    assert_eq!(
        rejection(
            "def apply[T](value: T) -> i64:
    f = def(x: i64) -> i64:
        missing + x
    f(1)

def main() -> ():
    n = apply(1)
"
        ),
        "7:9: cannot instantiate `apply[i64]`: its definition is rejected
  3:9: note: in instantiation `apply[i64]`: unknown binding `missing`"
    );
}

#[test]
fn expanding_recursion_keeps_instance_names_readable() {
    let error = rejection(
        "def f[T](x: T) -> Result[(), AllocError]:
    f[list[T]]([x]?)

def main() -> Result[(), AllocError]:
    f[i64](1)
",
    );
    let nested = format!("f[{}", "list[".repeat(19));
    assert_eq!(
        error,
        format!(
            "5:5: cannot instantiate `f[i64]`: its definition is rejected
  2:16: note: in instantiation `{}...`: type nesting exceeds the implementation limit of 64",
            &nested[..96]
        )
    );
}

#[test]
fn only_checks_of_an_instance_body_move_to_the_use() {
    // A body is checked only per instance, so an error in it that no type
    // argument causes is still found through a use. It is reported like any
    // other, and its note keeps the definition position and message.
    let body = "def keep[T](value: T) -> T:
    missing
    value
";
    plenty::check_source(&format!("{body}\ndef main() -> ():\n    pass\n")).unwrap();
    assert_eq!(
        rejection(&format!("{body}\ndef main() -> ():\n    x = keep(1)\n")),
        "6:9: cannot instantiate `keep[i64]`: its definition is rejected
  2:5: note: in instantiation `keep[i64]`: unknown binding `missing`"
    );
    // Declaration checks need no instance and stay at the definition.
    assert_eq!(
        rejection(
            "def keep[T: Missing](value: T) -> T:\n    value\n\ndef main() -> ():\n    x = keep(1)\n"
        ),
        "1:13: unknown type `Missing`"
    );
    // A use that does not satisfy the declaration never reaches the body.
    assert_eq!(
        rejection(
            "def add[T: IntType](a: T, b: T) -> T:\n    a + b\n\ndef main() -> ():\n    x = add('a', 'b')\n"
        ),
        "5:9: str does not satisfy IntType"
    );
}
