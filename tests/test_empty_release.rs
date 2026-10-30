//! Releasing an ownership slot that is empty on every path makes no runtime
//! call and no caller bookkeeping, and a slot that may be empty is tested
//! before both (issue #549). These tests run the command-line compiler, since
//! they read its `--dump asm` output.
mod support;

fn plenty(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

/// Stdout of `source` run with `args`, after checking it succeeded.
fn run(dir: &std::path::Path, args: &[&str]) -> String {
    let mut all = args.to_vec();
    all.push("p.plenty");
    let output = plenty(dir, &all);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// The `--dump asm` lines of the function whose section name starts with
/// `name`, compiled with `args`.
fn asm(dir: &std::path::Path, args: &[&str], name: &str) -> Vec<String> {
    let mut all = args.to_vec();
    all.extend(["--dump", "asm", "p.plenty", "-o", "out"]);
    let output = plenty(dir, &all);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(dir.join("out").join("asm.txt")).unwrap();
    let start = text
        .find(&format!("==== {name}"))
        .unwrap_or_else(|| panic!("{name} not dumped"));
    let rest = &text[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())]
        .lines()
        .map(str::to_string)
        .collect()
}

fn program(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), source).unwrap();
    dir
}

/// The issue #544 shape. Each constructor's frame slot is empty when it is
/// released: its value was moved out or never stored. A call between the
/// slot's last store and its release does not hide that.
const NESTED: &str = r#"from std.io import open_file, FileStream, StreamError

class Wrap:
    _file: File

    def out(self: Wrap) -> Outer:
        Outer(FileStream(self._file))

class Outer:
    _stream: FileStream

def cycle(file: File) -> File:
    w = Wrap(file)
    o = w.out()
    o._stream.file

def main() -> Result[(), AllocError | IoError | StreamError]:
    mut stream = open_file("/dev/null", "r")?
    mut file = stream.file
    for i in range(200):
        file = cycle(file)
    stream = FileStream(file)
    stream.close()?
    print("closed")?
    Ok(())
"#;

#[test]
fn empty_frame_slots_release_nothing() {
    let dir = program(NESTED);
    for args in [&[][..], &["--release"]] {
        assert_eq!(run(dir.path(), args), "closed\n");
        let cycle = asm(dir.path(), args, "cycle ");
        let runtime: Vec<_> = cycle
            .iter()
            .filter(|line| {
                [
                    "plenty_release",
                    "plenty_caller_enter",
                    "plenty_caller_leave",
                ]
                .iter()
                .any(|name| line.ends_with(name))
            })
            .collect();
        assert!(runtime.is_empty(), "{args:?}: {runtime:?}");
    }
}

/// `file` is moved on one path only, so its release at the join tests it
/// before naming the caller and calling the runtime.
const DYNAMIC: &str = r#"from std.io import open_file, FileStream, StreamError

def pick(file: File, keep: bool) -> Option[File]:
    mut out: Option[File] = Nothing
    if keep:
        out = Some(file)
    out

def main() -> Result[(), AllocError | IoError | StreamError]:
    stream = open_file("/dev/null", "r")?
    match pick(stream.file, True):
        case Some(file):
            mut again = FileStream(file)
            again.close()?
            print("kept")?
        case Nothing:
            print("lost")?
    other = open_file("/dev/null", "r")?
    print(pick(other.file, False).is_some())?
    Ok(())
"#;

/// The destination of a zero-test branch in function-relative dump offsets.
/// x86 prints absolute function offsets; A64 prints a signed PC displacement.
fn zero_branch_target(line: &str) -> Option<usize> {
    let (address, instruction) = line.trim().split_once(':')?;
    let address = usize::from_str_radix(address, 16).ok()?;
    let mut words = instruction.split_whitespace();
    if !matches!(words.next()?, "je" | "jz" | "cbz" | "b.eq") {
        return None;
    }
    let operand = words.next_back()?;
    if let Some(relative) = operand.strip_prefix("$+0x") {
        address.checked_add(usize::from_str_radix(relative, 16).ok()?)
    } else if let Some(relative) = operand.strip_prefix("$-0x") {
        address.checked_sub(usize::from_str_radix(relative, 16).ok()?)
    } else {
        usize::from_str_radix(operand.strip_prefix("0x")?, 16).ok()
    }
}

#[test]
fn possibly_empty_release_tests_before_bookkeeping() {
    let dir = program(DYNAMIC);
    for args in [&[][..], &["--release"]] {
        assert_eq!(run(dir.path(), args), "kept\nFalse\n");
    }
    let pick = asm(dir.path(), &["--release"], "pick ");
    let release = pick
        .iter()
        .position(|line| line.ends_with("plenty_release"))
        .unwrap_or_else(|| panic!("{pick:#?}"));
    let enter = pick[..release]
        .iter()
        .rposition(|line| line.ends_with("plenty_caller_enter"))
        .unwrap_or_else(|| panic!("{pick:#?}"));
    let leave = pick[release + 1..]
        .iter()
        .find(|line| line.ends_with("plenty_caller_leave"))
        .unwrap_or_else(|| panic!("{pick:#?}"));
    let leave_address = usize::from_str_radix(leave.trim().split_once(':').unwrap().0, 16).unwrap();
    let target = pick[..enter]
        .iter()
        .rev()
        .find_map(|line| zero_branch_target(line))
        .unwrap_or_else(|| panic!("{pick:#?}"));
    // Empty slots skip caller_enter, release, and caller_leave together.
    // Checking the actual destination avoids an ISA-dependent instruction count.
    assert!(target > leave_address, "{pick:#?}");
}

/// Destructors run once each whether a slot is moved out, reassigned in a
/// loop, or left empty on one path only. The output matches the compiler
/// before issue #549.
const DESTRUCTORS: &str = r#"class Guard:
    name: str
    def __del__(self: &mut Guard) -> ():
        print("drop", self.name).unwrap()

class Wrap:
    _items: list[Guard]

    def out(self: Wrap) -> Outer:
        Outer(Inner(self._items, 0))

class Inner:
    items: list[Guard]
    count: i64

class Outer:
    _inner: Inner

class Pair:
    first: Guard
    second: Guard

def guard(name: str) -> Guard:
    print("make", name).unwrap()
    Guard(name)

def show(g: &Guard) -> ():
    print("see", g.name).unwrap()

def cycle(items: list[Guard]) -> list[Guard]:
    w = Wrap(items)
    o = w.out()
    o._inner.items

def first(p: Pair) -> Guard:
    p.first

def rebuild(n: i64) -> Guard:
    mut p = Pair(guard("x"), guard("y"))
    for i in range(n):
        g = p.first
        show(&g)
        p = Pair(guard("a"), guard("b"))
    p.second

def maybe(keep: bool) -> Option[Guard]:
    g = guard("maybe")
    mut out: Option[Guard] = Nothing
    if keep:
        out = Some(g)
    out

def main() -> Result[(), AllocError | IoError]:
    mut items = [Guard("l1"), Guard("l2")]?
    for i in range(3):
        items = cycle(items)
    print(len(items))?
    f = first(Pair(guard("f1"), guard("f2")))
    show(&f)
    r = rebuild(2)
    show(&r)
    print(maybe(True).is_some(), maybe(False).is_some())?
    print("end")?
    Ok(())
"#;

const DESTRUCTORS_OUTPUT: &str = "2\nmake f1\nmake f2\ndrop f2\nsee f1\nmake x\nmake y\nsee x\n\
make a\nmake b\ndrop y\ndrop x\nsee a\nmake a\nmake b\ndrop b\ndrop a\ndrop a\nsee b\n\
make maybe\nmake maybe\ndrop maybe\nTrue False\ndrop maybe\nend\ndrop b\ndrop f1\n\
drop l1\ndrop l2\n";

#[test]
fn destructors_run_once() {
    let dir = program(DESTRUCTORS);
    for args in [&[][..], &["--release"]] {
        assert_eq!(run(dir.path(), args), DESTRUCTORS_OUTPUT, "{args:?}");
    }
}
