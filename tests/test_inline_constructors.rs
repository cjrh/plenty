//! Optimized builds lower small constructors, a class's `new` and its
//! `__init__`, at their call sites, and a field's first assignment in
//! `__init__` releases no old value (issue #548). Only `--release` builds
//! inline, so these tests run the command-line compiler both ways.
mod support;

fn plenty(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

/// Exit status, stdout and stderr of `source` run with `args`.
fn run(source: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), source).unwrap();
    let mut all = args.to_vec();
    all.push("p.plenty");
    let output = plenty(dir.path(), &all);
    (
        output.status.code(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// The user functions called by the function whose dump section name starts
/// with `name`.
fn calls(dir: &std::path::Path, name: &str) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join("out").join("asm.txt")).unwrap();
    let start = text
        .find(&format!("==== {name}"))
        .unwrap_or_else(|| panic!("{name} not dumped"));
    let rest = &text[start + 5..];
    let section = &rest[..rest.find("\n==== ").unwrap_or(rest.len())];
    section
        .lines()
        .filter_map(|line| line.rsplit_once("# __plenty_fn_"))
        .map(|(_, callee)| callee.trim().to_string())
        .collect()
}

/// Failure inside `__init__`, reassigned fields, argument order, generic and
/// recursive constructors, and a trap inside an inlined `__init__`.
const PROGRAM: &str = r#"class Guard:
    name: str
    def __del__(self: &mut Guard) -> ():
        print("drop", self.name).unwrap()

def guard(name: str) -> Guard:
    print("make", name).unwrap()
    Guard(name)

def refuse(name: str) -> Result[Guard, AllocError]:
    print("refuse", name).unwrap()
    Err(AllocError.CapacityOverflow)

class Pair:
    first: Guard
    second: Guard
    def __init__(self, a: Guard, fail: bool) -> Result[(), AllocError]:
        self.first = a
        if fail:
            self.second = refuse("second")?
        else:
            self.second = guard("second")
        Ok(())

class Swaps:
    held: Guard
    def __init__(self, n: i64, flip: bool) -> ():
        if flip:
            self.held = guard("then")
        else:
            self.held = guard("else")
        for i in range(n):
            self.held = guard("loop")
        self.held = guard("last")

class Ordered:
    left: Guard
    right: Guard

class Cell[T]:
    item: T

class Depth:
    value: i64
    def __init__(self, n: i64) -> Result[(), OverflowError[i64]]:
        if n == 0:
            self.value = 0
        else:
            self.value = Depth(n - 1)?.value + 1
        Ok(())

class Ratio:
    value: i64
    def __init__(self, a: i64, b: i64) -> ():
        # This fixture intentionally checks the constructor trap path.
        self.value = unchecked:
            a // b

def build(fail: bool) -> Result[Pair, AllocError]:
    Pair(guard("a"), fail)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    match build(True):
        case Ok(p):
            print("unexpected")?
        case Err(e):
            print("failed", str.repr(e)?)?
    p = build(False)?
    print(p.first.name, p.second.name)?
    s = Swaps(2, True)
    print(s.held.name)?
    t = Swaps(0, False)
    print(t.held.name)?
    o = Ordered(guard("l"), guard("r"))
    print(o.left.name, o.right.name)?
    c = Cell(guard("cell"))
    print(c.item.name, Cell(7).item)?
    print(Depth(5)?.value)?
    mut total = 0
    for i in range(1, 4):
        total += Ratio(12, i).value
    print(total)?
    print(Ratio(1, total - 22).value)?
    Ok(())
"#;

const EXPECTED: &str = "make a\nrefuse second\ndrop a\nfailed AllocError.CapacityOverflow\n\
make a\nmake second\na second\nmake then\nmake loop\ndrop then\nmake loop\ndrop loop\n\
make last\ndrop loop\nlast\nmake else\nmake last\ndrop else\nlast\nmake l\nmake r\nl r\n\
make cell\ncell 7\n5\n22\n";

#[test]
fn inlined_constructors_keep_cleanup_order_and_traps() {
    let debug = run(PROGRAM, &[]);
    let release = run(PROGRAM, &["--release"]);
    assert_eq!(debug.1, EXPECTED, "{}", debug.2);
    assert_eq!(debug.0, Some(1));
    let (line, source) = PROGRAM
        .lines()
        .enumerate()
        .find(|(_, line)| line.trim() == "a // b")
        .unwrap();
    let column = source.find('a').unwrap() + 1;
    assert!(
        debug
            .2
            .contains(&format!("p.plenty:{}:{column}: division by zero", line + 1)),
        "{}",
        debug.2
    );
    assert_eq!(release, debug);
}

/// The issue #544 reproduction: building `Wrap` and `Outer` calls neither
/// constructor, the recursive `Depth` constructor stays a call, and
/// `__init__` fills its fields without releasing them.
const NESTED: &str = r#"from std.io import open_file, FileStream, StreamError

class Wrap:
    _file: File

    def out(self: Wrap) -> Outer:
        Outer(FileStream(self._file))

class Outer:
    _stream: FileStream

class Named:
    label: str
    count: i64
    def __init__(self, label: str, count: i64) -> ():
        self.label = label
        self.count = count

class Depth:
    value: i64
    def __init__(self, n: i64) -> Result[(), OverflowError[i64]]:
        if n == 0:
            self.value = 0
        else:
            self.value = Depth(n - 1)?.value + 1
        Ok(())

def cycle(file: File) -> File:
    w = Wrap(file)
    o = w.out()
    o._stream.file

def named(label: str) -> i64:
    Named(label, 3).count

def main() -> Result[(), AllocError | IoError | StreamError | OverflowError[i64]]:
    mut stream = open_file("/dev/null", "r")?
    mut file = stream.file
    for i in range(200):
        file = cycle(file)
    stream = FileStream(file)
    stream.close()?
    print(named("n"), Depth(3)?.value)?
    Ok(())
"#;

#[test]
fn small_constructors_are_lowered_in_their_callers() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), NESTED).unwrap();
    let args = ["--release", "--dump", "ops,asm", "p.plenty", "-o", "out"];
    let output = plenty(dir.path(), &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ran = plenty(dir.path(), &["--release", "p.plenty"]);
    assert_eq!(String::from_utf8(ran.stdout).unwrap(), "3 3\n");
    for name in ["cycle ", "Wrap.out ", "named "] {
        let calls = calls(dir.path(), name);
        assert!(
            calls
                .iter()
                .all(|callee| !callee.ends_with(".new") && !callee.ends_with(".__init__")),
            "{name}: {calls:?}"
        );
    }
    let depth = calls(dir.path(), "Depth.__init__ ");
    assert!(
        depth.iter().any(|callee| callee.contains("Depth.")),
        "{depth:?}"
    );
    // The label's first assignment has no previous owner to release.
    let ops = std::fs::read_to_string(dir.path().join("out").join("ops.txt")).unwrap();
    let start = ops
        .find("==== Named.__init__")
        .expect("Named.__init__ dumped");
    let section = &ops[start..];
    let section = &section[..section[5..]
        .find("\n==== ")
        .map_or(section.len(), |e| e + 5)];
    assert!(section.contains("FillRef"), "{section}");
    assert!(!section.contains("WriteRef"), "{section}");
}

#[test]
fn generalized_errors_release_partial_fields_arguments_locals_and_payload_once() {
    // Extra scalar writes force the initializer to stay a native call rather
    // than depending only on the optimizer's small-constructor path.
    for large in [false, true] {
        let padding = if large {
            "        self.number = 0\n".repeat(80)
        } else {
            String::new()
        };
        let source = format!(
            r#"class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
enum Problem:
    Owned(Guard)
class Owner:
    first: Guard
    second: Guard
    number: i64
    def __init__(self, first: Guard, second: Guard, stage: i64, error: Problem) -> Result[(), Problem | OverflowError[i64]]:
        local = Guard("local")
        if stage == 0:
            return Err(error)
        self.first = first
        if stage == 1:
            return Err(error)
        self.second = second
        self.number = 0
{padding}        if stage == 2:
            return Err(error)
        self.number = i64.MAX + stage
        Ok(())
    def __del__(self) -> ():
        print("outer hook").unwrap()
def main() -> Result[(), IoError]:
    for stage in range(4):
        print("begin", stage)?
        result = Owner(Guard("first"), Guard("second"), stage, Problem.Owned(Guard("payload")))
        print("returned")?
        match result:
            case Err(error):
                drop(error)
                print("failed")?
            case Ok(_):
                print("unexpected")?
    Ok(())
"#
        );
        for args in [&[][..], &["--release"][..]] {
            let (status, stdout, stderr) = run(&source, args);
            assert_eq!(status, Some(0), "{stderr}\n{source}");
            assert!(!stdout.contains("outer hook"), "{stdout}");
            assert!(!stdout.contains("unexpected"), "{stdout}");
            for stage in 0..4 {
                let begin = format!("begin {stage}\n");
                let section = stdout.split_once(&begin).unwrap().1;
                let section = section.split("begin ").next().unwrap();
                for name in ["first", "second", "local", "payload", "returned", "failed"] {
                    assert_eq!(
                        section.lines().filter(|line| *line == name).count(),
                        1,
                        "large={large}, args={args:?}, stage={stage}, {name}: {stdout}"
                    );
                }
                // An affine error returned to the caller must not be freed
                // along with initializer locals or the partial instance.
                if stage < 3 {
                    assert!(
                        section.find("returned").unwrap() < section.find("payload").unwrap(),
                        "{stdout}"
                    );
                }
            }
        }
    }
}

#[test]
fn precise_scalar_errors_use_both_inline_and_native_constructor_abis() {
    for large in [false, true] {
        let padding = if large {
            "        self.value = 0\n".repeat(80)
        } else {
            String::new()
        };
        let source = format!(
            r#"class Offset:
    value: i64
    def __init__(self, value: i64, step: i64) -> Result[(), OverflowError[i64]]:
{padding}        self.value = value + step
        Ok(())
def build(n: i64, step: i64) -> Result[Offset, OverflowError[i64]]:
    Offset.new(n, step)
def main() -> Result[(), IoError]:
    for result in [build(i64.MAX, 1), build(i64.MIN, -1)].unwrap():
        match result:
            case Err(error):
                print(error.wrapped)?
            case Ok(_):
                print("unexpected")?
    print(build(2, 3).unwrap().value)?
    Ok(())
"#
        );
        for args in [&[][..], &["--release"][..]] {
            let result = run(&source, args);
            assert_eq!(result.0, Some(0), "{}", result.2);
            assert_eq!(result.1, "-9223372036854775808\n9223372036854775807\n5\n");
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("p.plenty"), source).unwrap();
        let result = plenty(
            dir.path(),
            &["--release", "--dump", "asm", "p.plenty", "-o", "out"],
        );
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let called = calls(dir.path(), "build ");
        assert_eq!(
            called.iter().any(|name| name.contains("Offset.")),
            large,
            "{called:?}"
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn constructor_fault_injection_releases_default_field_and_error_payload_owners() {
    for budget in 0..=3 {
        let source = format!(
            r#"class Guard:
    name: str
    values: list[i64]
    def __del__(self) -> ():
        print(self.name).unwrap()
enum Problem:
    Allocation(AllocError)
    Rejected(Guard)
def guard(name: str) -> Result[Guard, Problem]:
    values = list[i64].new().map_error(Problem.Allocation)?
    Ok(Guard(name, values))
class Owner:
    first: Guard
    second: Guard
    def __init__(self, given: Guard, first: Guard = guard("default")?, payload: Guard = guard("payload")?) -> Result[(), Problem]:
        print("init").unwrap()
        self.first = given
        self.second = guard("field")?
        Err(Problem.Rejected(payload))
    def __del__(self) -> ():
        print("outer hook").unwrap()
def main() -> Result[(), IoError]:
    given = guard("given").unwrap()
    print("__test_fail_allocations_after_{budget}__")?
    result = Owner(given)
    print("__test_restore_allocations__")?
    print("returned")?
    drop(result)
    Ok(())
"#
        );
        for args in [&[][..], &["--release"][..]] {
            let (status, stdout, stderr) = run(&source, args);
            assert_eq!(status, Some(0), "{stderr}\n{source}");
            assert!(!stdout.contains("outer hook"), "{stdout}");
            for (name, count) in [
                ("given", 1),
                ("default", usize::from(budget >= 1)),
                ("payload", usize::from(budget >= 2)),
                ("field", usize::from(budget >= 3)),
                ("init", usize::from(budget >= 2)),
            ] {
                assert_eq!(
                    stdout.lines().filter(|line| *line == name).count(),
                    count,
                    "budget={budget}, args={args:?}, {name}: {stdout}"
                );
            }
            let returned = stdout.find("returned").unwrap();
            if budget == 3 {
                assert!(stdout.find("payload").unwrap() > returned, "{stdout}");
            }
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn precise_constructor_overflow_does_not_allocate_in_either_native_mode() {
    let source = r#"class Offset:
    value: i64
    def __init__(self, n: i64, step: i64) -> Result[(), OverflowError[i64]]:
        self.value = n + step
        Ok(())
def main() -> Result[(), IoError]:
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    positive: Result[Offset, OverflowError[i64]] = Offset(i64.MAX, 1)
    negative: Result[Offset, OverflowError[i64]] = Offset.new(i64.MIN, -1)
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    match positive:
        case Err(error):
            print(error.wrapped)?
        case Ok(_):
            print("unexpected")?
    match negative:
        case Err(error):
            print(error.wrapped)?
        case Ok(_):
            print("unexpected")?
    Ok(())
"#;
    for args in [&[][..], &["--release"][..]] {
        let (status, stdout, stderr) = run(source, args);
        assert_eq!(status, Some(0), "{stderr}");
        let visible = stdout
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect::<Vec<_>>();
        assert_eq!(visible, ["-9223372036854775808", "9223372036854775807"]);
    }
}

#[test]
fn affine_error_payload_survives_inline_and_native_constructor_returns() {
    for large in [false, true] {
        let padding = if large {
            "        self.value = 0\n".repeat(80)
        } else {
            String::new()
        };
        let source = format!(
            r#"class Payload:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
class Refused:
    value: i64
    def __init__(self, error: Payload) -> Result[(), Payload]:
{padding}        return Err(error)
    def __del__(self) -> ():
        print("unexpected outer hook").unwrap()
def build(error: Payload) -> Result[Refused, Payload]:
    Refused(error)
def main() -> Result[(), IoError]:
    result = build(Payload("payload"))
    print("returned")?
    drop(result)
    Ok(())
"#
        );
        for args in [&[][..], &["--release"][..]] {
            let (status, stdout, stderr) = run(&source, args);
            assert_eq!(status, Some(0), "{stderr}");
            assert_eq!(stdout, "returned\npayload\n");
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("p.plenty"), source).unwrap();
        let output = plenty(
            dir.path(),
            &["--release", "--dump", "asm", "p.plenty", "-o", "out"],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let called = calls(dir.path(), "build ");
        assert_eq!(
            called.iter().any(|name| name.contains("Refused.")),
            large,
            "{called:?}"
        );
    }
}

#[test]
fn whole_output_parameters_preserve_affine_errors_in_inline_and_native_calls() {
    for large in [false, true] {
        let padding = if large {
            "        self.value = 0\n".repeat(80)
        } else {
            String::new()
        };
        let source = format!(
            r#"class Payload:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
type Init = Result[(), Payload]
class Refused[R]:
    value: i64
    def __init__(self, error: Payload) -> R:
{padding}        return Err(error)
    def __del__(self) -> ():
        print("unexpected outer hook").unwrap()
def build(error: Payload) -> Result[Refused[Init], Payload]:
    Refused[Init].new(error)
def main() -> Result[(), IoError]:
    result = build(Payload("payload"))
    print("returned")?
    drop(result)
    Ok(())
"#
        );
        for args in [&[][..], &["--release"][..]] {
            let (status, stdout, stderr) = run(&source, args);
            assert_eq!(status, Some(0), "{stderr}");
            assert_eq!(stdout, "returned\npayload\n");
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("p.plenty"), source).unwrap();
        let output = plenty(
            dir.path(),
            &["--release", "--dump", "asm", "p.plenty", "-o", "out"],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let called = calls(dir.path(), "build ");
        assert_eq!(
            called.iter().any(|name| name.contains("Refused[")),
            large,
            "{called:?}"
        );
    }
}
