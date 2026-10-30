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
    // A chain in which no template repeats is no longer than the number of
    // templates, which extends the depth limit (issue #435).
    let mut source = String::new();
    for i in 0..1000 {
        writeln!(
            source,
            "def step_{i}[T](value: T) -> T:\n    step_{}(value)",
            i + 1
        )
        .unwrap();
    }
    source.push_str(
        "def step_1000[T](value: T) -> T:\n    value\ndef main() -> ():\n    drop(step_0(0))\n",
    );
    plenty::check_source(&source).unwrap();
    // A cycle back to a cached instance does not add a dependency level.
    plenty::check_source(&source.replace(
        "def step_1000[T](value: T) -> T:\n    value",
        "def step_1000[T](value: T) -> T:\n    step_0(value)",
    ))
    .unwrap();
}

#[test]
fn a_chain_of_257_distinct_generic_functions_runs() {
    // Issue #435's reproduction, which reached the former depth limit of 256.
    let mut source = String::from("def h0[T: IntType](x: T) -> T:\n    x + 1\n");
    for i in 1..257 {
        source.push_str(&format!(
            "def h{i}[T: IntType](x: T) -> T:\n    x + h{}[T](x) + 1\n",
            i - 1
        ));
    }
    source.push_str("def main() -> i32:\n    i32(h256[i64](1) % 256)\n");
    let output = support::run(&source);
    // h0(1) = 2 and each step adds 2, so h256(1) = 514.
    assert_eq!(output.status.code(), Some(514 % 256), "{output:?}");
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
            error.contains("dependency depth limit of")
                && error
                    .contains("(256 plus one per generic function or generator consumer) exceeded"),
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
        "6:9: `twice[list[i64]]` copies `T`, but `list[i64]` cannot be copied
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
        "6:9: `sum[bool]` applies `+` to `T`, but `bool` does not support `+`
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
        "12:9: `outer[list[i64]]` requires `twice[list[i64]]`, which copies `T`, \
         but `list[i64]` cannot be copied
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
        "9:9: `Holder.both[list[i64]]` copies `T`, but `list[i64]` cannot be copied
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

/// A body rejected only because of a type argument names the argument at the
/// use, and its note leaves out advice for the body's author (issue #518).
#[test]
fn instance_errors_caused_by_a_type_argument_name_the_argument() {
    // A class method copying an element of a list of its parameter.
    assert_eq!(
        rejection(
            "class Holder[T]:
    items: list[T]

    def first(self) -> T:
        self.items[0]

def main() -> Result[(), AllocError]:
    holder = Holder([[1]?]?)
    item = holder.first()
    Ok(())
"
        ),
        "9:12: `Holder[list[i64]].first` copies `T`, but `list[i64]` cannot be copied
  5:9: note: in instantiation `Holder[list[i64]].first`: cannot move out of indexed storage"
    );
    // A generic function copying through a reference.
    assert_eq!(
        rejection(
            "def deref[T](value: &T) -> T:
    *value

def main() -> Result[(), AllocError]:
    items = [1]?
    copied = deref(&items)
    Ok(())
"
        ),
        "6:14: `deref[list[i64]]` copies `T`, but `list[i64]` cannot be copied
  2:5: note: in instantiation `deref[list[i64]]`: cannot move out of a reference"
    );
    // A class method required by a generic function names both instances.
    assert_eq!(
        rejection(
            "class Holder[T]:
    item: T

    def get(self) -> T:
        self.item

def read[U](holder: &Holder[U]) -> U:
    holder.get()

def main() -> Result[(), AllocError]:
    holder = Holder([1]?)
    item = read(&holder)
    Ok(())
"
        ),
        "12:12: `read[list[i64]]` requires `Holder[list[i64]].get`, which copies `T`, \
         but `list[i64]` cannot be copied
  5:9: note: in instantiation `Holder[list[i64]].get`: cannot move out of a field"
    );
    // A method the argument does not have, on a class and on a built-in type.
    let size = "def size[T](value: &T) -> i64:
    value.size()

class Point:
    x: i64
";
    assert_eq!(
        rejection(&format!(
            "{size}\ndef main() -> ():\n    n = size(&Point(1))\n"
        )),
        "8:9: `size[Point]` calls `size` on `T`, but `Point` has no method `size`
  2:5: note: in instantiation `size[Point]`: unknown method `size` on Point"
    );
    assert_eq!(
        rejection(&format!("{size}\ndef main() -> ():\n    n = size(&1)\n")),
        "8:9: `size[i64]` calls `size` on `T`, but `i64` has no method `size`
  2:5: note: in instantiation `size[i64]`: unsupported method `size`"
    );
}

/// A body error is attributed to a type argument only when the rejected type
/// is exactly one parameter's argument (issue #518).
#[test]
fn instance_errors_not_caused_by_one_type_argument_keep_the_instance_form() {
    // The copied list is a concrete field type, not the argument `i64`.
    assert_eq!(
        rejection(
            "class Holder[T]:
    item: T
    rows: list[list[i64]]

    def row(self) -> list[i64]:
        self.rows[0]

def main() -> Result[(), AllocError]:
    holder = Holder(1, [[2]?]?)
    row = holder.row()
    Ok(())
"
        ),
        "10:11: cannot instantiate `Holder[i64].row`: its definition is rejected
  6:9: note: in instantiation `Holder[i64].row`: cannot move out of indexed storage; \
         borrow it with &collection[index], take it with replace(collection[index], value) \
         or collection.pop(index), or copy it with copy(collection[index])?"
    );
    // Two parameters with the copied argument: either could be the cause.
    assert_eq!(
        rejection(
            "def both[K, V](key: &K, value: &V) -> tuple[K, V]:
    (*key, *value)

def main() -> Result[(), AllocError]:
    keys = [1]?
    values = [2]?
    pair = both(&keys, &values)
    Ok(())
"
        ),
        "7:12: cannot instantiate `both[list[i64], list[i64]]`: its definition is rejected
  2:6: note: in instantiation `both[list[i64], list[i64]]`: cannot move out of a reference; \
         use the reference instead of dereferencing it, or copy the value with copy(reference)?"
    );
}

/// A type the definition writes itself is not blamed on a parameter whose
/// argument it equals (issue #519).
#[test]
fn instance_errors_on_types_the_definition_writes_keep_the_instance_form() {
    // A field type names the copied list; the argument `list[i64]` is not
    // what the method copies.
    assert_eq!(
        rejection(
            "class Holder[T]:
    item: T
    rows: list[list[i64]]

    def row(self) -> list[i64]:
        self.rows[0]

def main() -> Result[(), AllocError]:
    holder = Holder([1]?, [[2]?]?)
    row = holder.row()
    Ok(())
"
        ),
        "10:11: cannot instantiate `Holder[list[i64]].row`: its definition is rejected
  6:9: note: in instantiation `Holder[list[i64]].row`: cannot move out of indexed storage; \
         borrow it with &collection[index], take it with replace(collection[index], value) \
         or collection.pop(index), or copy it with copy(collection[index])?"
    );
    // A parameter type of a generic function.
    assert_eq!(
        rejection(
            "def pick[T](value: T, rows: list[list[i64]]) -> list[i64]:
    rows[0]

def main() -> Result[(), AllocError]:
    row = pick([1]?, [[2]?]?)
    Ok(())
"
        ),
        "5:11: cannot instantiate `pick[list[i64]]`: its definition is rejected
  2:5: note: in instantiation `pick[list[i64]]`: cannot move out of indexed storage; \
         borrow it with &collection[index], take it with replace(collection[index], value) \
         or collection.pop(index), or copy it with copy(collection[index])?"
    );
    // An annotation in the body, rejected by the ownership check.
    assert_eq!(
        rejection(
            "def twice[T](value: T) -> Result[(), AllocError]:
    row: list[i64] = [1]?
    first = row
    second = row
    Ok(())

def main() -> Result[(), AllocError]:
    twice([1]?)?
    Ok(())
"
        ),
        "8:5: cannot instantiate `twice[list[i64]]`: its definition is rejected
  4:14: note: in instantiation `twice[list[i64]]`: use of moved binding `row`
  3:13: note: `row` is moved here"
    );
    // An unrelated concrete annotation must not suppress a genuine T value.
    assert_eq!(
        rejection(
            "def twice[T](value: T) -> Result[(), AllocError]:
    row: list[i64] = [1]?
    first = value
    second = value
    Ok(())

def main() -> Result[(), AllocError]:
    twice([1]?)?
    Ok(())
"
        ),
        "8:5: `twice[list[i64]]` copies `T`, but `list[i64]` cannot be copied
  4:14: note: in instantiation `twice[list[i64]]`: use of moved binding `value`
  3:13: note: `value` is moved here"
    );
}

#[test]
fn inferred_concrete_values_are_not_blamed_on_matching_type_parameters() {
    let cases = [
        (
            "def inferred[T](value: T) -> Result[(), AllocError]:\n    row = [1]?\n    first = row\n    second = row\n    Ok(())\n\ndef main() -> Result[(), AllocError]:\n    inferred([0]?)?\n    Ok(())\n",
            "inferred[list[i64]]",
            "use of moved binding `row`",
        ),
        (
            "def inferred[T](value: T) -> Result[(), AllocError]:\n    rows = [[1]?]?\n    row = rows[0]\n    Ok(())\n\ndef main() -> Result[(), AllocError]:\n    inferred([0]?)?\n    Ok(())\n",
            "inferred[list[i64]]",
            "cannot move out of indexed storage",
        ),
        (
            "def inferred[T](value: T) -> bool:\n    flag = True\n    flag + flag\n\ndef main() -> ():\n    drop(inferred(False))\n",
            "inferred[bool]",
            "operator `+` does not accept bool",
        ),
        (
            "def inferred[T](value: T) -> i64:\n    count = 1\n    count.size()\n\ndef main() -> ():\n    drop(inferred(42))\n",
            "inferred[i64]",
            "unsupported method `size`",
        ),
        (
            "def inferred[T](value: T, flag: bool) -> Result[(), AllocError]:\n    mut row = value\n    if flag:\n        row = [1]?\n    first = row\n    second = row\n    Ok(())\n\ndef main() -> Result[(), AllocError]:\n    inferred([0]?, True)?\n    Ok(())\n",
            "inferred[list[i64]]",
            "use of moved binding `row`",
        ),
        (
            "def inferred[T](key: T, values: &dict[T, bool]) -> bool:\n    values[key] + values[key]\n\ndef main() -> Result[(), AllocError]:\n    values = {True: False}?\n    drop(inferred(True, &values))\n    Ok(())\n",
            "inferred[bool]",
            "operator `+` does not accept bool",
        ),
        (
            "def inferred[T, U](items: &T, other: U) -> bool:\n    items[0] + items[0]\n\ndef main() -> Result[(), AllocError]:\n    items = [True]?\n    drop(inferred(&items, False))\n    Ok(())\n",
            "inferred[list[bool], bool]",
            "operator `+` does not accept bool",
        ),
    ];
    for (source, instance, reason) in cases {
        let error = rejection(source);
        assert!(
            error.contains(&format!(
                "cannot instantiate `{instance}`: its definition is rejected"
            )),
            "{source}\n{error}"
        );
        assert!(error.contains(reason), "{source}\n{error}");
    }
}

#[test]
fn parameter_values_keep_their_origin_through_aliases_and_projections() {
    let comparison = rejection(
        "def before[T](value: T) -> bool:\n    value < value\n\ndef main() -> ():\n    drop(before(True))\n",
    );
    assert!(
        comparison.contains("`before[bool]` applies `<` to `T`"),
        "{comparison}"
    );
    for source in [
        "def actual[T](value: T) -> Result[(), AllocError]:\n    concrete = [1]?\n    alias = value\n    first = alias\n    second = alias\n    Ok(())\n\ndef main() -> Result[(), AllocError]:\n    actual([0]?)?\n    Ok(())\n",
        "def actual[T](value: T) -> Result[(), AllocError]:\n    rows = [value]?\n    item = rows[0]\n    Ok(())\n\ndef main() -> Result[(), AllocError]:\n    actual([0]?)?\n    Ok(())\n",
        "def actual[T](values: &list[T]) -> T:\n    values[0]\n\ndef main() -> Result[(), AllocError]:\n    values = [[0]?]?\n    drop(actual(&values))\n    Ok(())\n",
    ] {
        let error = rejection(source);
        assert!(
            error.contains("`actual[list[i64]]` copies `T`, but `list[i64]` cannot be copied"),
            "{source}\n{error}"
        );
    }
}

/// A field counts only for the bodies that access it, so a method that copies
/// `T` is still blamed on it when another field writes the argument's type, as
/// `LruCache[K, V].get` relies on (issue #519).
#[test]
fn fields_a_method_does_not_access_keep_the_parameter_blame() {
    assert_eq!(
        rejection(
            "class Holder[T]:
    item: T
    rows: list[list[i64]]

    def get(self) -> T:
        self.item

def main() -> Result[(), AllocError]:
    holder = Holder([1]?, [[2]?]?)
    item = holder.get()
    Ok(())
"
        ),
        "10:12: `Holder[list[i64]].get` copies `T`, but `list[i64]` cannot be copied
  6:9: note: in instantiation `Holder[list[i64]].get`: cannot move out of a field"
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

// Templates are released during lowering once nothing left to lower names
// them (issue #434). Each template here is reached only after some of its
// callers are lowered, or through a path the release count must follow:
// template chains, mutual recursion, function values, named arguments,
// defaults, generic class methods, closures, f-strings, and a local
// indexed by a name that could read as a type argument.
const RELEASE_PROGRAM: &str = r#"from std.fmt import FormatError

def ident[T](value: T) -> T:
    value

def twice[T](value: T) -> (T, T):
    (ident(value), ident(value))

def deep[T](value: T) -> T:
    shallow(value)

def shallow[T](value: T) -> T:
    ident(value)

def is_even[T](n: i64, tag: T) -> bool:
    if n == 0:
        return True
    is_odd(n - 1, tag)

def is_odd[T](n: i64, tag: T) -> bool:
    if n == 0:
        return False
    is_even(n - 1, tag)

def apply[F: Callable[[i64], i64]](f: &F, value: i64) -> i64:
    f(value)

def pick[T](first: T, second: T, take_second: bool = False) -> T:
    if take_second:
        return second
    first

def offset(value: i64 = ident(40)) -> i64:
    value + 2

def describe[T](value: T) -> i64:
    add = def [value](extra: i64) -> i64:
        ident(extra)
    add(1)

class Holder[T]:
    item: T

    def get(self) -> T:
        ident(self.item)

def early() -> i64:
    deep(5)

def main() -> Result[(), AllocError | IoError | FormatError]:
    print(early())?
    print(twice(3))?
    print(is_even(10, "x"), is_odd(7, 1.5))?
    print(apply(&ident[i64], 9))?
    f = ident[i64]
    print(f(11))?
    print(pick(1, 2, take_second=True), pick(second="b", first="a"))?
    print(offset(), offset(1))?
    print(describe("s"))?
    print(Holder(7).get())?
    T = 3
    xs = [10, 20, 30, 40]?
    print(xs[T], f"{ident(4)} {deep('d')}"?)?
    print(deep(6))?
    Ok(())
"#;

#[test]
fn templates_reached_after_their_callers_are_lowered_still_instantiate() {
    let output = support::assert_success(support::run_file(RELEASE_PROGRAM));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "5\n(3, 3)\nTrue True\n9\n11\n2 a\n42 3\n1\n7\n40 4 d\n6\n"
    );
}

#[test]
fn templates_of_an_imported_module_instantiate_after_its_functions_are_lowered() {
    let dir = tempfile::tempdir().unwrap();
    support::write_file(
        &dir.path().join("pairs.plenty"),
        "pub def wrap[T](value: T) -> (T, T):\n    (inner(value), inner(value))\n\n\
         pub def inner[T](value: T) -> T:\n    value\n\n\
         pub def later() -> i64:\n    inner(8)\n",
    );
    let executable = support::compile_file(
        dir.path(),
        "import pairs\n\n\
         def main() -> Result[(), IoError]:\n    print(pairs.later())?\n    \
         print(pairs.wrap(\"w\"))?\n    print(pairs.inner(2.5))?\n    Ok(())\n",
    );
    let output = support::assert_success(std::process::Command::new(executable).output().unwrap());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "8\n(\"w\", \"w\")\n2.5\n"
    );
}

// An instance's body is substituted when it is lowered, not when it is
// instantiated; once its template is released, the last instance takes the
// template's body instead of copying it (issue #436). Each template here has
// instances lowered both before and after that release, with type parameters
// in local annotations, explicit type arguments, nested generic calls,
// generator factories and generator consumers.
const DEFERRED_BODY_PROGRAM: &str = r#"def keep[T](value: T) -> T:
    copy: T = value
    copy

def nest[T](value: T) -> (T, T):
    inner: T = keep[T](value)
    (inner, keep(value))

def once[T](value: T) -> Generator[T]:
    copy: T = value
    yield copy

def collect[T](source: Generator[T]) -> Result[list[T], AllocError]:
    [item for item in source]

def first() -> (i64, i64):
    nest(1)

def second() -> (str, str):
    nest("b")

def main() -> Result[(), AllocError | IoError]:
    print(first())?
    print(second())?
    print(nest(2.5))?
    print(collect(once(7u8))?)?
    print(collect(once("g"))?)?
    Ok(())
"#;

#[test]
fn instance_bodies_are_substituted_when_lowered() {
    let output = support::assert_success(support::run_file(DEFERRED_BODY_PROGRAM));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "(1, 1)\n(\"b\", \"b\")\n(2.5, 2.5)\n[7]\n[\"g\"]\n"
    );
}

#[test]
fn type_parameter_misuse_in_a_body_is_reported_at_the_requiring_use() {
    assert_eq!(
        rejection(
            "def wrap[T](value: T) -> i64:
    copy: T[i64] = value
    1

def main() -> ():
    x = wrap(3)
"
        ),
        "6:9: cannot instantiate `wrap[i64]`: its definition is rejected
  2:11: note: in instantiation `wrap[i64]`: a type parameter cannot take type arguments"
    );
}
