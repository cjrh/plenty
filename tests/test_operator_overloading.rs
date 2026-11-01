//! Static arithmetic hooks preserve ordinary call, ownership and error contracts.
mod support;
use support::assert_stdout as expect;

#[test]
fn binary_unary_and_inplace_hooks_borrow_operands() {
    expect(
        r#"
class Vector:
    x: i64
    def __add__(self, other: &Vector) -> Vector:
        Vector(self.x + other.x)
    def __sub__(self, other: &Vector) -> i64:
        self.x - other.x
    def __mul__(self, scale: i64) -> Vector:
        Vector(self.x * scale)
    def __rmul__(self, scale: i64) -> Vector:
        Vector(scale * self.x)
    def __neg__(self) -> Vector:
        Vector(-self.x)
    def __pos__(self) -> i64:
        self.x
    def __invert__(self) -> i64:
        ~self.x
    def __iadd__(self: &mut Vector, other: &Vector) -> ():
        self.x += other.x
mut a = Vector(3)
b = Vector(4)
print(a + b, a * 2, 2 * a, -a, +a, ~a).unwrap()
difference: i64 = a - b
a += b
print(a, b, difference).unwrap()
print((a + b) + Vector(1), -(a + b)).unwrap()
"#,
        "Vector(x=7) Vector(x=6) Vector(x=6) Vector(x=-3) 3 -4\nVector(x=7) Vector(x=4) -1\nVector(x=12) Vector(x=-11)\n",
    );
}

#[test]
fn reflected_and_inplace_operations_preserve_evaluation_order() {
    expect(
        r#"
class V:
    n: i64
    def __rsub__(self, other: i64) -> i64:
        other - self.n
    def __iadd__(self: &mut V, other: i64) -> ():
        self.n += other
def left() -> i64:
    print("left").unwrap()
    10
def right() -> V:
    print("right").unwrap()
    V(3)
def index() -> i64:
    print("index").unwrap()
    0
print(left() - right()).unwrap()
mut rows = [V(1)].unwrap()
rows[index()] += left()
print(rows).unwrap()
"#,
        "left\nright\n7\nindex\nleft\n[V(n=11)]\n",
    );
}

#[test]
fn fallible_hooks_propagate_and_temporaries_are_destroyed_once() {
    expect(
        r#"
class V:
    n: i64
    def __add__(self, other: &V) -> Result[V, str]:
        if other.n < 0:
            return Err("negative")
        Ok(V(self.n + other.n))
    def __del__(self: &mut V) -> ():
        print("drop", self.n).unwrap()
def identity[T](value: T) -> T:
    value
def sum() -> Result[(), str]:
    result = (identity(V(1)) + identity(V(2)))?
    print(result.n).unwrap()
    drop((V(4) + V(-1))?)
    Ok(())
print(str.repr(sum()).unwrap()).unwrap()
"#,
        "drop 2\ndrop 1\n3\ndrop -1\ndrop 4\ndrop 3\nResult[(), str].Err(\"negative\")\n",
    );
}

#[test]
fn generic_classes_enums_and_inferred_calls_dispatch_statically() {
    expect(
        r#"
class V[T]:
    n: T
    def __add__(self, other: &V[T]) -> T:
        self.n + other.n
enum Flag:
    On
    Off
    def __invert__(self) -> bool:
        match self:
            case Flag.On:
                False
            case Flag.Off:
                True
def identity[T](value: T) -> T:
    value
def at(xs: &list[i64], index: i64) -> &i64:
    &xs[identity(index) + 0]
def add[T](a: &T, b: &T) -> i64:
    a + b
a = V[i64](2)
b = V[i64](4)
print(a + b, add(a, b), identity(V[i64](5)) + b, ~Flag.Off).unwrap()
xs = [12].unwrap()
print(*at(xs, 0)).unwrap()
"#,
        "6 6 9 True\n12\n",
    );
}

#[test]
fn every_existing_arithmetic_token_reaches_its_hook() {
    for (op, hook) in [
        ("/", "truediv"),
        ("//", "floordiv"),
        ("%", "mod"),
        ("&", "and"),
        ("|", "or"),
        ("^", "xor"),
        ("<<", "lshift"),
        (">>", "rshift"),
    ] {
        expect(
            &format!(
                "class V:\n    n: i64\n    def __{hook}__(self, other: i64) -> i64:\n        self.n + other\n    def __r{hook}__(self, other: i64) -> i64:\n        other - self.n\n    def __i{hook}__(self: &mut V, other: i64) -> ():\n        self.n += other\nmut v = V(3)\nprint(v {op} 5, 5 {op} v).unwrap()\nv {op}= 5\nprint(v.n).unwrap()"
            ),
            "8 2\n8\n",
        );
    }
}

#[test]
fn invalid_hooks_and_mutability_are_rejected() {
    for (source, message) in [
        (
            "class V:\n    def __add__(self) -> i64:\n        1",
            "exactly",
        ),
        (
            "class V:\n    def __neg__(self: &mut V) -> i64:\n        1",
            "shared self",
        ),
        (
            "class V:\n    def __add__(self, other: V) -> i64:\n        1",
            "copyable",
        ),
        (
            "class V:\n    def __iadd__(self: &mut V, other: i64) -> Result[(), str]:\n        Ok(())",
            "must return ()",
        ),
        (
            "class V:\n    def __neg__(self) -> &V:\n        self",
            "owned value",
        ),
        (
            "class V:\n    def __add__[T](self, other: T) -> i64:\n        1",
            "type parameters",
        ),
        (
            "class V:\n    def __add__(self, other: i64 = 1) -> i64:\n        other",
            "defaults",
        ),
        (
            "class V:\n    def __iadd__(self: &mut V, other: i64) -> ():\n        pass\nv = V()\nv += 1",
            "immutable",
        ),
        (
            "class V:\n    def __add__(self, other: i64) -> i64:\n        other\nmut v = V()\nv += 1",
            "__iadd__",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(message), "{source}\n{error}");
    }
}

#[test]
fn result_types_and_literal_context_come_from_the_selected_hook() {
    expect(
        r#"
class V:
    x: f64
    def __mul__(self, other: i64) -> f64:
        self.x * f64(other)
    def __radd__(self, other: i32) -> f64:
        f64(other) + self.x
def identity[T](v: T) -> T:
    v
v = V(2.5)
print((v * 2i64) + 0.5, 1 + identity(V(2.5))).unwrap()
"#,
        "5.5 3.5\n",
    );
}

#[test]
fn equality_stays_structural_and_logical_operators_stay_boolean() {
    expect(
        "class V:\n    n: i64\na = V(1)\nb = V(1)\nprint(a == b, [a].unwrap() == [b].unwrap()).unwrap()",
        "True True\n",
    );
    assert!(support::check_source("class V:\n    def __and__(self, other: &V) -> bool:\n        True\na = V()\nb = V()\nprint(a and b).unwrap()").is_err());
}

#[test]
fn visibility_and_loans_are_not_bypassed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("values.plenty"), "pub class V:\n    pub n: i64\n    def __add__(self, other: &V) -> i64:\n        self.n + other.n\n").unwrap();
    std::fs::write(dir.path().join("main.plenty"), "from values import V\ndef main() -> Result[(), IoError]:\n    a = V(1)\n    b = V(2)\n    print(a + b)?\n    Ok(())\n").unwrap();
    let error = plenty::check_file(&dir.path().join("main.plenty"), None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("private"), "{error}");
    for source in [
        "class V:\n    n: i64\n    def __add__(self, other: i64) -> i64:\n        self.n + other\ndef mutate(v: &mut V) -> i64:\n    v.n = 9\n    1\nmut v = V(2)\nprint(v + mutate(&mut v)).unwrap()",
        "class V:\n    n: i64\n    def __iadd__(self: &mut V, other: &V) -> ():\n        self.n += other.n\nmut v = V(2)\nv += v",
        "class V:\n    n: i64\n    def __iadd__(self: &mut V, other: i64) -> ():\n        self.n += other\nmut v = V(2)\nborrow = &v\nv += 1\nprint(borrow).unwrap()",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}

#[test]
fn operator_and_named_calls_emit_identical_code_and_small_hooks_inline() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("p.plenty"),
        r#"
class Scalar:
    x: f64
    def __add__(self, other: &Scalar) -> f64:
        self.x + other.x
def overloaded(a: &Scalar, b: &Scalar) -> f64:
    a + b
def named(a: &Scalar, b: &Scalar) -> f64:
    a.__add__(b)
def fields(a: &Scalar, b: &Scalar) -> f64:
    a.x + b.x
def main() -> Result[(), IoError]:
    a = Scalar(1.25)
    b = Scalar(2.5)
    print(overloaded(a, b), named(a, b), fields(a, b))?
    Ok(())
"#,
    )
    .unwrap();
    for release in [false, true] {
        let mut command = std::process::Command::new(support::plenty_bin());
        command.current_dir(dir.path()).env("PLENTY_CACHE", "off");
        if release {
            command.arg("--release");
        }
        let output = command
            .args(["--dump", "asm", "p.plenty", "-o", "out"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = std::fs::read_to_string(dir.path().join("out/asm.txt")).unwrap();
        let section = |name: &str| -> Vec<String> {
            let body = text
                .split("==== ")
                .find(|part| part.starts_with(&format!("{name} ")))
                .unwrap();
            body.lines()
                .skip(1)
                .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
                .map(str::to_string)
                .collect()
        };
        assert_eq!(section("overloaded"), section("named"));
        if release {
            assert_eq!(section("overloaded"), section("fields"));
            assert!(
                !section("overloaded")
                    .iter()
                    .any(|line| line.contains("call") || line.contains("jmp"))
            );
        }
    }
}

#[test]
fn inlined_checked_arithmetic_keeps_trap_location_and_tail_cleanup_order() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), "class V:\n    n: i64\n    def __add__(self, other: &V) -> i64:\n        self.n + other.n\nclass Guard:\n    def __del__(self: &mut Guard) -> ():\n        print(\"drop\").unwrap()\ndef wrapper(a: &V, b: &V) -> i64:\n    guard = Guard()\n    a + b\ndef main() -> ():\n    a = V(i64.MAX)\n    b = V(1)\n    print(wrapper(a, b)).unwrap()\n").unwrap();
    for mode in [&[][..], &["--release"][..]] {
        let output = std::process::Command::new(support::plenty_bin())
            .current_dir(dir.path())
            .env("PLENTY_CACHE", "off")
            .args(mode)
            .arg("p.plenty")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "drop\n");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("p.plenty:4:") && error.contains("overflow"),
            "{error}"
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn arithmetic_dispatch_needs_no_heap_allocation() {
    expect(
        r#"
class V:
    n: i64
    def __add__(self, other: &V) -> V:
        V(self.n + other.n)
    def __iadd__(self: &mut V, other: i64) -> ():
        self.n += other
mut a = V(1)
b = V(2)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
a += 3
c = a + b
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(c.n).unwrap()
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n6\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocating_hooks_report_failure_without_consuming_the_inputs() {
    expect(
        r#"
class Data:
    values: list[i64]
    def __add__(self, other: &Data) -> Result[Data, AllocError]:
        mut result = copy(&self.values)?
        for value in &other.values:
            result.append(*value)?
        Ok(Data(result))
a = Data([1].unwrap())
b = Data([2].unwrap())
print("__test_fail_allocations_after_0__").unwrap()
result = a + b
print("__test_restore_allocations__").unwrap()
match result:
    case Ok(value):
        print("unexpected").unwrap()
    case Err(error):
        print("allocation refused").unwrap()
print(a.values, b.values).unwrap()
print((a + b).unwrap().values).unwrap()
"#,
        "__test_fail_allocations_after_0__\n__test_restore_allocations__\nallocation refused\n[1] [2]\n[1, 2]\n",
    );
}

// Phase guard (issue #613): equality, ordering and keys never call user code,
// so the Python hooks are rejected rather than ignored. B37 (`__cmp__`) and
// B74 (user-defined keys) replace this rejection for the hooks they admit.
#[test]
fn comparison_and_hashing_hooks_are_rejected() {
    for (hook, rule) in [
        ("__eq__", "`==` and `!=` always compare fields structurally"),
        ("__ne__", "`==` and `!=` always compare fields structurally"),
        ("__lt__", "come only from `@newtype` forwarding"),
        ("__le__", "come only from `@newtype` forwarding"),
        ("__gt__", "come only from `@newtype` forwarding"),
        ("__ge__", "come only from `@newtype` forwarding"),
        ("__cmp__", "come only from `@newtype` forwarding"),
        ("__hash__", "hash structurally"),
        ("__key__", "hash structurally"),
    ] {
        let class = format!(
            "class Money:\n    cents: i64\n    def {hook}(self, other: &Money) -> bool:\n        True\nprint(Money(1) == Money(2)).unwrap()"
        );
        let enumeration = format!(
            "enum Coin:\n    Penny\n    def {hook}(self) -> bool:\n        True\nprint(Coin.Penny == Coin.Penny).unwrap()"
        );
        let protocol = format!(
            "protocol Ordered:\n    def {hook}(self) -> bool:\n        pass\nprint(1).unwrap()"
        );
        for source in [class, enumeration, protocol] {
            let error = support::check_source(&source).unwrap_err().to_string();
            assert!(
                error.contains(&format!("`{hook}` is not supported")) && error.contains(rule),
                "{source}\n{error}"
            );
            assert!(error.contains("Equality, ordering and hashing"), "{error}");
        }
    }
    // A generic class's methods follow the same rule.
    let error = support::check_source(
        "class Cell[T]:\n    value: T\n    def __eq__(self, other: &Cell[T]) -> bool:\n        True\nprint(Cell(1) == Cell(2)).unwrap()",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("`__eq__` is not supported"), "{error}");
}

// Every `__name__` method that is not a hook Plenty calls is rejected, so a
// Python special method cannot compile and then be ignored (issue #645).
#[test]
fn unsupported_special_methods_are_rejected() {
    for (hook, advice) in [
        ("__str__", "`display` and `debug`"),
        ("__repr__", "`display` and `debug`"),
        ("__format__", "`display` and `debug`"),
        ("__pow__", "`base.pow(exponent)`"),
        ("__iter__", "generator method"),
        ("__next__", "generator method"),
        ("__bool__", "predicate method"),
        ("__getitem__", "named methods"),
        ("__setitem__", "named methods"),
        ("__matmul__", "Plenty never calls it"),
        ("__call__", "Plenty never calls it"),
        ("__anything__", "Plenty never calls it"),
    ] {
        assert_special_method_rejected(hook, advice);
    }
}

// Phase guard (issue #645): `len()` and `in` on user types are planned under
// B49 (issue #623); delete this when they call a user method.
#[test]
fn length_and_membership_hooks_are_rejected() {
    for hook in ["__len__", "__contains__"] {
        assert_special_method_rejected(hook, "planned under B49");
    }
}

fn assert_special_method_rejected(hook: &str, advice: &str) {
    let class = format!(
        "class Bag:\n    count: i64\n    def {hook}(self) -> bool:\n        True\nprint(Bag(1).count).unwrap()"
    );
    let generic = format!(
        "class Cell[T]:\n    value: T\n    def {hook}(self) -> bool:\n        True\nprint(Cell(1).value).unwrap()"
    );
    let enumeration = format!(
        "enum Coin:\n    Penny\n    def {hook}(self) -> bool:\n        True\nprint(Coin.Penny == Coin.Penny).unwrap()"
    );
    let protocol =
        format!("protocol Sized:\n    def {hook}(self) -> bool:\n        pass\nprint(1).unwrap()");
    for source in [class, generic, enumeration, protocol] {
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains(&format!("`{hook}` is not supported"))
                && error.contains(advice)
                && error.contains("Reserved method names"),
            "{source}\n{error}"
        );
    }
}

#[test]
fn names_with_underscores_on_one_side_are_ordinary_methods() {
    expect(
        "class Bag:\n    count: i64\n    def __size(self) -> i64:\n        self.count\n    def size__(self) -> i64:\n        self.count\n    def _len_(self) -> i64:\n        self.count\nbag = Bag(2)\nprint(bag.__size(), bag.size__(), bag._len_()).unwrap()",
        "2 2 2\n",
    );
}

#[test]
fn enums_cannot_declare_context_hooks() {
    for hook in ["__enter__", "__exit__"] {
        let source = format!(
            "enum Coin:\n    Penny\n    def {hook}(self) -> ():\n        pass\nprint(1).unwrap()"
        );
        let error = support::check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains(&format!("reserved enum method `{hook}`")),
            "{error}"
        );
    }
}
