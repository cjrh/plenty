//! Numeric module constants share ordinary module visibility and name binding.
use std::process::Command;

#[test]
fn constants_import_as_values_and_locals_can_shadow_them() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("numbers.plenty"),
        "pub const count: i32 = 3\npub const angle: f32 = f32.PI\nconst hidden: u64 = u64.MAX\npub def largest() -> u64:\n    hidden\n").unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(&main, "import numbers as n\nfrom numbers import count as three\nfrom std.math import pi as circle\nconst small: i8 = -128i8\nconst negative_inf: f64 = -f64.INF\ndef main() -> Result[(), Failure]:\n    print(n.count + three)?\n    print(n.angle == f32.PI)?\n    print(n.largest() == u64.MAX)?\n    print(circle == f64.PI)?\n    print(small)?\n    print(negative_inf)?\n    three = 10\n    print(three)?\n    Ok(())\n").unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, None).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"6\nTrue\nTrue\nTrue\n-128\n-inf\n10\n");
}

#[test]
fn constant_privacy_and_callability_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("numbers.plenty"),
        "pub const value: f64 = f64.PI\nconst hidden: i64 = 3\n",
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    for (source, message) in [
        (
            "import numbers\ndef main() -> ():\n    print(numbers.hidden).unwrap()\n",
            "private",
        ),
        (
            "from numbers import hidden\ndef main() -> ():\n    pass\n",
            "private",
        ),
        (
            "import numbers\ndef main() -> ():\n    numbers.value()\n",
            "not a callable",
        ),
        (
            "from numbers import value\ndef main() -> ():\n    value()\n",
            "not a callable",
        ),
        (
            "import numbers\ndef main() -> ():\n    numbers.value = 0.0\n",
            "assignment",
        ),
    ] {
        std::fs::write(&main, source).unwrap();
        let error = plenty::check_file(&main, None).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn constants_reject_executable_initializers_type_mismatches_and_collisions() {
    for (source, message) in [
        ("const bad: f64 = print(1)\n", "constant initializer"),
        ("const bad: f64 = 1.0 + 2.0\n", "constant initializer"),
        ("const bad: list[i64] = [1]\n", "numeric type"),
        ("const bad: i64 = f64.PI\n", "expected"),
        ("const bad: u8 = 256u8\n", "range"),
        ("const bad: f64 = f64.UNKNOWN\n", "unknown constant"),
        (
            "const value: i64 = 1\nconst value: i64 = 2\n",
            "duplicate declaration",
        ),
        (
            "def value() -> i64:\n    1\nconst value: i64 = 2\n",
            "duplicate declaration",
        ),
        (
            "const value: i64 = 1\ndef value() -> i64:\n    2\n",
            "duplicate declaration",
        ),
        (
            "const value: i64 = 1\ntype value = i64\n",
            "duplicate declaration",
        ),
    ] {
        let source = format!("{source}def main() -> ():\n    pass\n");
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
}
