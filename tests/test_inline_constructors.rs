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
    def __init__(self, n: i64) -> ():
        if n == 0:
            self.value = 0
        else:
            self.value = Depth(n - 1).value + 1

class Ratio:
    value: i64
    def __init__(self, a: i64, b: i64) -> ():
        self.value = a // b

def build(fail: bool) -> Result[Pair, AllocError]:
    Pair(guard("a"), fail)

def main() -> Result[(), AllocError | IoError]:
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
    print(Depth(5).value)?
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
    assert!(
        debug.2.contains("p.plenty:54:22: division by zero"),
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
    def __init__(self, n: i64) -> ():
        if n == 0:
            self.value = 0
        else:
            self.value = Depth(n - 1).value + 1

def cycle(file: File) -> File:
    w = Wrap(file)
    o = w.out()
    o._stream.file

def named(label: str) -> i64:
    Named(label, 3).count

def main() -> Result[(), AllocError | IoError | StreamError]:
    mut stream = open_file("/dev/null", "r")?
    mut file = stream.file
    for i in range(200):
        file = cycle(file)
    stream = FileStream(file)
    stream.close()?
    print(named("n"), Depth(3).value)?
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
