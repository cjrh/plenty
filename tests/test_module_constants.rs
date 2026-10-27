//! Module constants: visibility and name binding, compile-time evaluation of
//! initializers, static assertions, and included files.
use std::process::Command;

#[test]
fn constants_import_as_values_and_locals_can_shadow_them() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("numbers.plenty"),
        "pub const count: i32 = 3\npub const angle: f32 = f32.PI\nconst hidden: u64 = u64.MAX\npub def largest() -> u64:\n    hidden\n").unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(&main, "import numbers as n\nfrom numbers import count as three\nfrom std.math import pi as circle\nconst small: i8 = -128i8\nconst negative_inf: f64 = -f64.INF\ndef main() -> Result[(), IoError]:\n    print(n.count + three)?\n    print(n.angle == f32.PI)?\n    print(n.largest() == u64.MAX)?\n    print(circle == f64.PI)?\n    print(small)?\n    print(negative_inf)?\n    three = 10\n    print(three)?\n    Ok(())\n").unwrap();
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
        ("const bad: f64 = print(1)\n", "not a constant expression"),
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

/// Compiles and runs `source`, a complete program, returning its output.
fn run(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(&main, source).unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, None).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn constant_expressions_fold_with_the_runtime_operator_rules() {
    let output = run("\
const KIB: i64 = 1024
const MIB: i64 = KIB * KIB
const LATER: i64 = EARLY + 1
const EARLY: i64 = 41
const FLOOR: i64 = -7 // 2
const MODULO: i64 = -7 % 3
const NEGATIVE_DIVISOR: i64 = 7 % -3
const MASK: u8 = ~(1u8 << 3)
const HIGH: u8 = 15u8 if False else 240u8
const SIGNED_SHIFT: i8 = i8.MIN >> 2
const DISCARDED: i8 = 3i8 << 7
const BITS: u16 = (255u16 & 15u16) | 256u16 ^ 1u16
const HALF: f64 = 1.0 / 2.0
const THIRD: f32 = f32(1) / 3.0
const TRUNCATED: u8 = u8(255.9)
const WIDE: i64 = i64(u8.MAX) + i64(-2.5)
const NAME: str = \"plenty\" + \"-\" + \"lang\"
const LARGE: bool = MIB > KIB and not (KIB == 0)
const INF: f64 = -f64.INF
const NEGATIVE_ZERO: f64 = -0.0
const SMALLEST: i64 = i64.MIN

def main() -> Result[(), AllocError | IoError]:
    print(MIB)?
    print(LATER)?
    print(FLOOR)?
    print(MODULO)?
    print(NEGATIVE_DIVISOR)?
    print(MASK)?
    print(HIGH)?
    print(SIGNED_SHIFT)?
    print(DISCARDED)?
    print(BITS)?
    print(HALF)?
    print(THIRD == f32(1) / 3.0)?
    print(TRUNCATED)?
    print(WIDE)?
    print(NAME)?
    print(LARGE)?
    print(INF)?
    print(NEGATIVE_ZERO)?
    print(SMALLEST)?
    Ok(())
");
    assert_eq!(
        output,
        "1048576\n42\n-4\n2\n-2\n247\n240\n-32\n-128\n271\n0.5\nTrue\n255\n253\nplenty-lang\nTrue\n-inf\n-0.0\n-9223372036854775808\n"
    );
}

#[test]
fn constants_from_other_modules_take_part_in_expressions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sizes.plenty"),
        "pub const PAGE: i64 = 4096\npub const PREFIX: str = \"size\"\n",
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        "import sizes\nfrom sizes import PAGE\nconst PAGES: i64 = 3\nconst TOTAL: i64 = PAGE * PAGES + sizes.PAGE // 2\nconst LABEL: str = sizes.PREFIX + \": \"\nassert TOTAL == 14336, \"pages changed\"\ndef main() -> Result[(), IoError]:\n    print(LABEL)?\n    print(TOTAL)?\n    Ok(())\n",
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, None).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert_eq!(output.stdout, b"size: \n14336\n");
}

#[test]
fn constant_evaluation_errors_are_compile_time_errors() {
    for (source, message) in [
        (
            "const A: i64 = B\nconst B: i64 = A\n",
            "constant `A` depends on itself: A -> B -> A",
        ),
        ("const A: i64 = A + 1\n", "depends on itself: A -> A"),
        ("const A: u8 = 200 + 100\n", "overflows u8"),
        ("const A: i64 = i64.MAX + 1\n", "overflows i64"),
        ("const A: i64 = i64.MIN // -1\n", "overflows i64"),
        ("const A: u8 = 1u8 - 2u8\n", "overflows u8"),
        ("const A: i64 = 1 // 0\n", "division by zero"),
        ("const A: i64 = 1 % 0\n", "division by zero"),
        (
            "const A: i64 = 1 << 64\n",
            "shift count 64 is outside 0..64",
        ),
        ("const A: u8 = 1u8 >> 8u8\n", "shift count 8"),
        ("const A: u8 = -1\n", "unsigned"),
        ("const A: u8 = u8(300)\n", "out of range for u8"),
        ("const A: u8 = u8(-0.5 - 1.0)\n", "out of range for u8"),
        ("const A: i64 = i64(f64.NAN)\n", "out of range for i64"),
        ("const A: i64 = 7 / 2\n", "`/` divides floats"),
        ("const A: f64 = 1.0 // 2.0\n", "does not apply to f64"),
        ("const A: f64 = 1.0 % 2.0\n", "does not apply to f64"),
        ("const A: i64 = 1 + 2u8\n", "cannot combine"),
        ("const A: i64 = 5u8\n", "expected i64, got u8"),
        ("const A: str = \"a\" + 1\n", "cannot combine"),
        ("const A: str = f\"{1}\"\n", "not a constant expression"),
        (
            "const A: i64 = 1 if True else 2.0\n",
            "conditional branches differ",
        ),
        ("const A: bool = 1 < 2.0\n", "cannot compare"),
        (
            "const A: str = include_str(\"x.txt\")\n",
            "require compiling a file",
        ),
    ] {
        let source = format!("{source}def main() -> ():\n    pass\n");
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(message), "{source}: {error}");
    }
}

/// As at run time, `x if c else y` evaluates only the selected branch and
/// `and`/`or` short-circuit, so a trap in the other operand is no error. The
/// other operand is still type-checked.
#[test]
fn unselected_operands_are_checked_but_not_evaluated() {
    let output = run("\
const DIVISOR: i64 = 0
const SAFE: i64 = 1 if DIVISOR == 0 else 100 // DIVISOR
const LARGE: u8 = u8(300) if False else 255u8
const NESTED: i64 = 0 if True else (1 if False else i64.MAX + 1)
const GUARDED: bool = DIVISOR != 0 and 100 // DIVISOR > 1
const EITHER: bool = DIVISOR == 0 or 1 << 64 == 0
const CAST: i64 = 7 if True else i64(f64.NAN)

def main() -> Result[(), IoError]:
    print(SAFE)?
    print(LARGE)?
    print(NESTED)?
    print(GUARDED)?
    print(EITHER)?
    print(CAST)?
    Ok(())
");
    assert_eq!(output, "1\n255\n0\nFalse\nTrue\n7\n");
    for (source, message) in [
        ("const A: i64 = 1 if True else 1 // 0\n", ""),
        ("const A: i64 = 1 // 0 if True else 1\n", "division by zero"),
        (
            "const A: i64 = 1 if True else 2.0\n",
            "conditional branches differ",
        ),
        ("const A: i64 = 1 if True else 1 + 2u8\n", "cannot combine"),
        (
            "const A: i64 = 1 if True else missing\n",
            "not a constant expression",
        ),
        ("const A: bool = False and 1\n", "expected bool, got i64"),
        ("const A: bool = True or 1 == 2.0\n", "cannot compare"),
    ] {
        let source = format!("{source}def main() -> ():\n    pass\n");
        let result = plenty::check_source(&source);
        if message.is_empty() {
            result.unwrap();
        } else {
            let error = result.unwrap_err().to_string();
            assert!(error.contains(message), "{source}: {error}");
        }
    }
}

#[test]
fn static_assertions_are_checked_at_compile_time() {
    let source = "const WIDTH: i64 = 64\nassert WIDTH % 8 == 0\nassert WIDTH > 8, \"too narrow\"\ndef main() -> ():\n    pass\n";
    plenty::check_source(source).unwrap();
    for (source, message) in [
        (
            "const WIDTH: i64 = 60\nassert WIDTH % 8 == 0\n",
            "static assertion failed",
        ),
        (
            "const WIDTH: i64 = 4\nassert WIDTH > 8, \"width \" + \"too small\"\n",
            "static assertion failed: width too small",
        ),
        ("assert 1\n", "expected bool, got i64"),
        ("assert True, 3\n", "expected str, got i64"),
        ("assert missing == 1\n", "not a constant expression"),
    ] {
        let source = format!("{source}def main() -> ():\n    pass\n");
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(message), "{source}: {error}");
    }
}

#[test]
fn included_files_become_constants() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/greeting.txt"), "héllo\n").unwrap();
    std::fs::write(dir.path().join("assets/data.bin"), [0u8, 0xFF, b'a']).unwrap();
    std::fs::create_dir(dir.path().join("lib")).unwrap();
    // A path is relative to the file that names it, not to the entry.
    std::fs::write(
        dir.path().join("lib/text.plenty"),
        "pub const GREETING: str = include_str(\"../assets/greeting.txt\")\n",
    )
    .unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(
        &main,
        "from lib.text import GREETING\nconst DATA: bytes = include_bytes(\"assets/data.bin\")\nconst BANNER: str = \"> \" + GREETING\ndef main() -> Result[(), IoError]:\n    print(BANNER)?\n    print(DATA)?\n    print(GREETING.byte_len())?\n    Ok(())\n",
    )
    .unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, None).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "> héllo\n\nb\"\\x00\\xffa\"\n7\n"
    );
}

#[test]
fn included_paths_are_checked() {
    let parent = tempfile::tempdir().unwrap();
    std::fs::write(parent.path().join("outside.txt"), "secret").unwrap();
    let dir = parent.path().join("project");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("latin1.txt"), [b'a', 0xE9]).unwrap();
    let main = dir.join("main.plenty");
    let absolute = parent.path().join("outside.txt");
    for (declaration, message) in [
        (
            format!(
                "const S: str = include_str({:?})",
                absolute.display().to_string()
            ),
            "must be relative".to_string(),
        ),
        (
            "const S: str = include_str(\"../outside.txt\")".into(),
            "outside the source root".into(),
        ),
        (
            "const S: str = include_str(\"missing.txt\")".into(),
            "reading `missing.txt`".into(),
        ),
        (
            "const S: str = include_str(\"latin1.txt\")".into(),
            "not valid UTF-8 at byte 1".into(),
        ),
        (
            "const S: bytes = include_str(\"latin1.txt\")".into(),
            "`include_str` initializes a `str` constant".into(),
        ),
        (
            "const S: str = include_bytes(\"latin1.txt\")".into(),
            "`include_bytes` initializes a `bytes` constant".into(),
        ),
        (
            "const S: str = include_str(\"a\" + \"b\")".into(),
            "one string literal path".into(),
        ),
    ] {
        std::fs::write(
            &main,
            format!("{declaration}\ndef main() -> ():\n    pass\n"),
        )
        .unwrap();
        let error = plenty::check_file(&main, None).unwrap_err().to_string();
        assert!(error.contains(&message), "{declaration}: {error}");
    }
}
