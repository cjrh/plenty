//! `@newtype` classes forward their field's operators (issue #555). Typing
//! stays nominal, explicit hooks win, and `@newtype` is independent of `@copy`.
mod support;
use support::assert_stdout as expect;

const WRAPPERS: &str = r#"
@newtype
class B:
    value: i64

@newtype
class C:
    value: i64
"#;

#[test]
fn arithmetic_bitwise_unary_and_comparisons_forward_to_the_field() {
    expect(
        &format!(
            "{WRAPPERS}{}",
            r#"
def make(n: i64) -> B:
    B(n)
a = B(12)
b = B(5)
print(a + b, a - b, a * b, a // b, a % b).unwrap()
print(a & b, a | b, a ^ b, a << B(2), a >> B(1)).unwrap()
print(-a, ~b, (a + b) + make(1), -(a - b)).unwrap()
print(a < b, a <= b, a > b, a >= b, a == B(12), a != b).unwrap()
mut c = B(1)
c += a
c *= B(2)
c -= b
print(c).unwrap()
"#
        ),
        "B(value=17) B(value=7) B(value=60) B(value=2) B(value=2)\n\
         B(value=4) B(value=13) B(value=9) B(value=48) B(value=6)\n\
         B(value=-12) B(value=-6) B(value=18) B(value=-7)\n\
         False False True True True True\n\
         B(value=21)\n",
    );
}

#[test]
fn result_shaped_fields_generics_nesting_and_places_forward() {
    expect(
        r#"
@newtype
class Name:
    text: str

@newtype
class W[T]:
    value: T

@newtype
class B:
    value: i64

@newtype
class Outer:
    inner: B

class Holder:
    b: B

class Vec2:
    x: f64
    y: f64
    def __add__(self, other: &Vec2) -> Vec2:
        Vec2(self.x + other.x, self.y + other.y)
    def __mul__(self, other: &Vec2) -> f64:
        self.x * other.x + self.y * other.y

@newtype
class Velocity:
    v: Vec2

full: Result[Name, AllocError] = Name("Ada") + Name(" Lovelace")
print(full.unwrap(), Name("a") < Name("b")).unwrap()
print(W(2) + W(3), W(1.5) * W(2.0), (W("a") + W("b")).unwrap()).unwrap()
print(Outer(B(1)) + Outer(B(2)), -Outer(B(3)), Outer(B(1)) < Outer(B(2))).unwrap()
mut h = Holder(B(1))
h.b += B(4)
mut xs = [B(1), B(2)].unwrap()
xs[1] *= B(10)
print(h, xs).unwrap()
v = Velocity(Vec2(1.0, 2.0))
print(v + v, v * v).unwrap()
"#,
        "Name(text=\"Ada Lovelace\") True\n\
         W[i64](value=5) W[f64](value=3.0) W[str](value=\"ab\")\n\
         Outer(inner=B(value=3)) Outer(inner=B(value=-3)) True\n\
         Holder(b=B(value=5)) [B(value=1), B(value=20)]\n\
         Velocity(v=Vec2(x=2.0, y=4.0)) 5.0\n",
    );
}

#[test]
fn explicit_hooks_override_forwarding_per_operator() {
    expect(
        r#"
@newtype
class Price:
    cents: i64
    def __add__(self, other: &Price) -> Price:
        Price(self.cents + other.cents + 1000)
    def __sub__(self, other: &Price) -> i64:
        self.cents - other.cents
    def __mul__(self, scale: i64) -> Price:
        Price(self.cents * scale)
    def __iadd__(self: &mut Price, other: i64) -> ():
        self.cents += other
mut p = Price(1) + Price(2)
p += 5
print(p, Price(5) - Price(2), Price(3) * 4, Price(9) // Price(2)).unwrap()
"#,
        "Price(cents=1008) 3 Price(cents=12) Price(cents=4)\n",
    );
}

#[test]
fn overflow_and_division_traps_come_from_the_wrapped_operation() {
    for (body, message) in [
        ("print(B(i64.MAX) + B(1)).unwrap()", "overflow"),
        ("print(B(1) // B(0)).unwrap()", "division by zero"),
    ] {
        let output = support::run(&format!("{WRAPPERS}{body}"));
        assert!(!output.status.success(), "{body}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(message), "{body}\n{error}");
    }
}

#[test]
fn typing_stays_nominal() {
    for (body, message) in [
        (
            "x = B(1) + 2",
            "forwards `+` only between two `B` values, not `B` and `i64`",
        ),
        (
            "x = 2 + B(1)",
            "forwards `+` only between two `B` values, not `i64` and `B`",
        ),
        (
            "x = B(1) + C(2)",
            "forwards `+` only between two `B` values, not `B` and `C`",
        ),
        ("x = B(1) < C(2)", "not `B` and `C`"),
        (
            "mut x = B(1)\nx += 1",
            "forwards `+=` only with another `B` value",
        ),
        ("mut x = B(1)\nx -= C(1)", "declare `__isub__`"),
        ("x: i64 = B(1) + B(2)", "expected i64"),
    ] {
        let error = support::check_source(&format!("{WRAPPERS}{body}"))
            .unwrap_err()
            .to_string();
        assert!(error.contains(message), "{body}\n{error}");
    }
}

#[test]
fn unsupported_field_operators_and_plain_classes_are_rejected() {
    for (source, message) in [
        (
            "@newtype\nclass L:\n    xs: list[i64]\na = L([1].unwrap())\nb = a + a",
            "operator `+` does not accept list[i64]",
        ),
        (
            "@newtype\nclass F:\n    x: f64\nb = F(1.0) % F(2.0)",
            "require integers",
        ),
        (
            "@newtype\nclass T:\n    flag: bool\nb = T(True) & T(False)",
            "does not accept bool",
        ),
        (
            "class P:\n    value: i64\nb = P(1) + P(2)",
            "annotate the one-field class `@newtype`",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(message), "{source}\n{error}");
    }
}

#[test]
fn newtype_does_not_imply_copy_and_borrows_operands() {
    expect(
        r#"
@newtype
class B:
    value: i64
a = B(3)
b = B(4)
print(a + a, a + b, a < b).unwrap()
print(a, b, -a).unwrap()
"#,
        "B(value=6) B(value=7) True\nB(value=3) B(value=4) B(value=-3)\n",
    );
    let error =
        support::check_source("@newtype\nclass B:\n    value: i64\na = B(1)\nb = a\nc = a + B(1)")
            .unwrap_err()
            .to_string();
    assert!(error.contains("moved"), "{error}");
}

#[test]
fn copy_and_newtype_compose_in_either_order() {
    for annotations in ["@copy\n@newtype", "@newtype\n@copy"] {
        expect(
            &format!(
                "{annotations}\nclass Meters:\n    value: f64\na = Meters(1.5)\nb = a\nc = a + b\nprint(a, b, c, a < c).unwrap()"
            ),
            "Meters(value=1.5) Meters(value=1.5) Meters(value=3.0) True\n",
        );
        let error = support::check_source(&format!(
            "{annotations}\nclass Names:\n    items: list[str]"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("which moves"), "{annotations}\n{error}");
    }
}

#[test]
fn private_fields_forward_across_modules() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("units.plenty"),
        "@newtype\npub class Secret:\n    value: i64\n\npub def secret(n: i64) -> Secret:\n    Secret(n)\n\npub def reveal(s: &Secret) -> i64:\n    s.value\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.plenty"),
        "from units import Secret, secret, reveal\n\ndef main() -> Result[(), IoError]:\n    total = secret(4) + secret(5)\n    print(reveal(&total), secret(1) < secret(2))?\n    Ok(())\n",
    )
    .unwrap();
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .arg("main.plenty")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "9 True\n");
}

#[test]
fn invalid_newtype_declarations_are_diagnosed() {
    for (source, message) in [
        (
            "@newtype\nclass P:\n    x: i64\n    y: i64",
            "must declare exactly one field, the wrapped value; it declares 2",
        ),
        (
            "@newtype\nclass E:\n    pass",
            "must declare exactly one field, the wrapped value; it declares 0",
        ),
        (
            "@newtype\nclass I:\n    x: i64\n    def __init__(self: &mut I, x: i64) -> ():\n        self.x = x",
            "cannot define `__init__`",
        ),
        (
            "@newtype\nenum Color:\n    Red",
            "applies only to class declarations",
        ),
        (
            "@newtype\ndef f() -> ():\n    pass",
            "applies only to class declarations",
        ),
        ("@newtype\n@newtype\nclass D:\n    x: i64", "duplicate"),
        (
            "@final\nclass U:\n    x: i64",
            "the annotations are `@copy`, `@newtype` and `@redacted`",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(message), "{source}\n{error}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn forwarding_scalar_fields_needs_no_heap_allocation() {
    expect(
        &format!(
            "{WRAPPERS}{}",
            r#"
mut a = B(1)
b = B(2)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
a += b
c = -(a + b)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(c.value).unwrap()
"#
        ),
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n-5\n",
    );
}
