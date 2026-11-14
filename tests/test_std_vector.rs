//! Vec3 keeps ordinary class ownership and scalar IEEE results in every profile.
mod support;
use std::process::Command;

fn run(source: &str, optimize: bool, cpu: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vector.plenty");
    std::fs::write(&path, source).unwrap();
    let mut cmd = Command::new(support::plenty_bin());
    cmd.env("PLENTY_CACHE", "off");
    if !optimize {
        cmd.arg("--fast-compile");
    }
    cmd.args(["--cpu", cpu]).arg(path);
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

const PROGRAM: &str = r#"from std.vector import Vec3
from std.test import check

def identity[T](value: T) -> T:
    value

def change(value: &mut f64) -> ():
    *value = 7.0

def recurrence(count: i64, seed: f64) -> Vec3:
    mut result = Vec3(seed, -seed, 0.5)
    step = Vec3(0.25, 0.5, -0.125)
    for i in range(count):
        result = (result + step) * 0.5
    result

def main() -> ():
    a = Vec3(1.0, 2.0, 3.0)
    b = Vec3(4.0, -5.0, 6.0)
    check((a + b).to_tuple() == (5.0, -3.0, 9.0), "add").unwrap()
    check((a - b).to_tuple() == (-3.0, 7.0, -3.0), "subtract").unwrap()
    check((-a).to_tuple() == (-1.0, -2.0, -3.0), "negate").unwrap()
    check((a * 2.0).to_tuple() == (2.0, 4.0, 6.0), "scale").unwrap()
    check((2.0 * a).to_tuple() == (2.0, 4.0, 6.0), "reverse scale").unwrap()
    check(a.mul(b).to_tuple() == (4.0, -10.0, 18.0), "component multiply").unwrap()
    check(a.dot(b) == 12.0, "ordered dot").unwrap()
    check(Vec3(3.0, 4.0, 0.0).length() == 5.0, "length").unwrap()
    check(a.lane(-1).unwrap_or(-99.0) == -99.0 and a.lane(3).unwrap_or(-99.0) == -99.0 and a.lane(2).unwrap() == 3.0, "lane").unwrap()
    mut copied = identity(a)
    change(&mut copied.y)
    check(copied.y == 7.0 and a.y == 2.0, "copy and addressable component").unwrap()
    fixed: array[Vec3, 2] = [a, b]
    values = [fixed[0], fixed[1]].unwrap()
    optional: Option[Vec3] = Some(values[1])
    result: Result[Vec3, str] = Ok(optional.unwrap())
    check(result.unwrap().to_tuple() == b.to_tuple(), "storage and wrappers").unwrap()
    transform = def(value: &Vec3, scale: f64) -> Vec3:
        value * scale
    check(transform(a, 3.0).to_tuple() == (3.0, 6.0, 9.0), "callable argument").unwrap()
    mut columns = clist[Vec3]()
    columns.append(a).unwrap()
    for x in columns.column(Vec3.x):
        check(x == 1.0, "ordinary scalar column").unwrap()
    captured = def [a](scale: f64) -> Vec3:
        a * scale
    check(captured(3.0).to_tuple() == (3.0, 6.0, 9.0), "explicit capture").unwrap()
    mut maybe: Option[Vec3] = Nothing
    for i in range(4):
        maybe = Some(Vec3(f64(i), 2.0, 3.0)) if i % 2 == 0 else Nothing
        selected = maybe.unwrap_or(Vec3(-1.0, -2.0, -3.0))
        print(selected.to_tuple()).unwrap()
    print(recurrence(10, 2.0).to_tuple()).unwrap()
    print("ok").unwrap()
"#;

#[test]
fn arithmetic_ownership_and_storage_match_scalar_profiles() {
    let expected = run(PROGRAM, false, "x86-64");
    assert!(expected.ends_with("ok\n"));
    assert_eq!(run(PROGRAM, true, "x86-64"), expected);
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        assert_eq!(run(PROGRAM, true, "x86-64-v3"), expected);
    }
}

const IEEE: &str = r#"from std.vector import Vec3
from std.test import check

def ieee(seed: f64) -> ():
    a = Vec3(-0.0, seed, 5e-324)
    scaled = a * f64.INF
    check(scaled.x != scaled.x and scaled.y == f64.INF and scaled.z == f64.INF, "infinite scale").unwrap()
    negated = -a
    check(1.0 / negated.x == f64.INF, "negative zero negation").unwrap()
    check(negated.y == -seed and negated.z == -5e-324, "negation").unwrap()
    ordered = Vec3(1e16, -1e16, seed)
    check(ordered.dot(Vec3(1.0, 1.0, 1.0)) == seed, "ordered reduction").unwrap()
    opposite = Vec3(f64.INF, 1.0, 2.0) - Vec3(f64.INF, 0.0, 0.0)
    check(opposite.x != opposite.x and opposite.y == 1.0 and opposite.z == 2.0, "independent components").unwrap()
    zero = Vec3(-0.0, 0.0, -0.0) * 2.0
    check(1.0 / zero.x == -f64.INF and 1.0 / zero.y == f64.INF and 1.0 / zero.z == -f64.INF, "signed scale").unwrap()

def main() -> ():
    ieee(2.0)
    print("ok").unwrap()
"#;

#[test]
fn ieee_components_and_ordered_reduction_match_profiles() {
    assert_eq!(run(IEEE, false, "x86-64"), "ok\n");
    assert_eq!(run(IEEE, true, "x86-64"), "ok\n");
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        assert_eq!(run(IEEE, true, "x86-64-v3"), "ok\n");
    }
}

#[test]
fn component_loan_prevents_owner_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("loan.plenty");
    std::fs::write(&path, "from std.vector import Vec3\ndef main() -> ():\n    mut a = Vec3(1.0, 2.0, 3.0)\n    x = &a.x\n    a = Vec3(4.0, 5.0, 6.0)\n    print(*x).unwrap()\n").unwrap();
    let error = plenty::check_file(&path, None).unwrap_err().to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[test]
#[cfg(target_arch = "x86_64")]
fn wide_negation_does_not_force_narrow_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("negation.plenty");
    std::fs::write(&source, "from std.vector import Vec3\ndef negate(x: f64, y: f64, z: f64) -> f64:\n    value = -Vec3(x, y, z)\n    value.dot(Vec3(1.0, 1.0, 1.0))\ndef main() -> ():\n    print(negate(1.0, 2.0, 3.0)).unwrap()\n").unwrap();
    let dump = dir.path().join("dump");
    let output = Command::new(support::plenty_bin())
        .args([
            "--dump",
            "asm",
            "--cpu",
            "x86-64-v3",
            "--dump-filter",
            "negate",
        ])
        .arg(&source)
        .arg("-o")
        .arg(&dump)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = std::fs::read_to_string(dump.join("asm.txt")).unwrap();
    assert!(asm.contains("==== negate"), "{asm}");
    assert!(!asm.contains("rebuilt-narrow"), "{asm}");
}
