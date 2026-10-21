//! Values whose ownership needs no runtime walk are copied, stored and dropped
//! without calling the runtime (issue #384): an environment with no managed
//! captures needs no cleanup, an empty ownership slot is not released, and a
//! sum whose payloads keep no internal addresses copies them directly.
mod support;
use support::assert_visible_stdout as native;

fn plenty(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: std::process::Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// The `plenty_collection` operation codes passed by the function whose dump
/// section name starts with `name`: each call follows the move of its code to
/// `%edi`.
fn dispatches(dir: &std::path::Path, name: &str) -> Vec<u32> {
    let text = std::fs::read_to_string(dir.join("out").join("asm.txt")).unwrap();
    let start = text
        .find(&format!("==== {name}"))
        .unwrap_or_else(|| panic!("{name} not dumped"));
    let rest = &text[start + 5..];
    let section = &rest[..rest.find("\n==== ").unwrap_or(rest.len())];
    let mut code = None;
    let mut calls = Vec::new();
    for line in section.lines() {
        if let Some((_, operand)) = line.split_once("mov     $0x")
            && let Some((hex, "%edi")) = operand.split_once(',')
        {
            code = u32::from_str_radix(hex, 16).ok();
        }
        if line.ends_with("# plenty_collection") {
            calls.push(code.expect("operation code before the call"));
        }
    }
    calls
}

const COPY_PAYLOAD: u32 = 117;

const PROGRAM: &str = r#"import std.bytes
import std.error
from std.bytes import Bytes, BytesError

enum Problem:
    Short
    Wrapped(AllocError)

enum Shape:
    Empty
    Box(i64, i64)
    Named(str, i64)

def lift[T](result: Result[T, AllocError]) -> Result[T, Problem]:
    std.error.map_error(result, def once (cause: AllocError) -> Problem: Problem.Wrapped(cause))

def scaled(factor: i64) -> Option[Closure[[i64], i64]]:
    Some(def [factor](value: i64) -> i64: value * factor)

def labelled(label: str) -> Result[OnceClosure[[i64], str], Problem]:
    Ok(def once [label](value: i64) -> str: (label + str.from(value).unwrap()).unwrap())

def filled(n: u8) -> Result[Bytes, BytesError]:
    mut out = std.bytes.empty()?
    for i in range(n):
        out.push(i)?
    Ok(out)

def span(n: i64) -> Option[range[i64]]:
    if n > 0:
        return Some(range(n))
    Nothing

def shapes(n: i64) -> Result[list[Shape], AllocError]:
    mut out: list[Shape] = []?
    out.append(Shape.Empty)?
    out.append(Shape.Box(n, n + 1))?
    out.append(Shape.Named(("shape" + str.from(n)?)?, n))?
    Ok(out)

def main() -> Result[(), AllocError | IoError | BytesError | Problem]:
    mut total = 0
    for i in range(1000):
        step: Result[i64, AllocError] = Ok(i)
        total += lift(step)?
    print(total)?
    failed: Result[i64, AllocError] = Err(AllocError.OutOfMemory)
    match lift(failed):
        case Err(Problem.Wrapped(cause)):
            print(str.repr(cause)?)?
        case _:
            print("unexpected")?
    match scaled(3):
        case Some(f):
            print(f(14))?
        case Nothing:
            print("none")?
    for i in range(200):
        convert = labelled(("item " + "")?)?
        moved = convert
        if i == 199:
            print(moved(7))?
    print("__test_small_live_heap__")?
    data = filled(5)?
    copied = data
    print(copied.len())?
    match span(4):
        case Some(r):
            mut sum = 0
            for i in r:
                sum += i
            print(sum)?
        case Nothing:
            print("empty")?
    match span(0):
        case Some(r):
            print("unexpected")?
        case Nothing:
            print("no span")?
    for shape in shapes(4)?:
        match shape:
            case Shape.Empty:
                print("empty")?
            case Shape.Box(w, h):
                print(w * h)?
            case Shape.Named(name, n):
                print(name, n)?
    Ok(())
"#;

const EXPECTED: &str =
    "499500\nAllocError.OutOfMemory\n42\nitem 7\n5\n6\nno span\nempty\n20\nshape4 4\n";

#[test]
fn unmanaged_environments_and_plain_payloads_keep_their_behaviour() {
    native(PROGRAM, EXPECTED);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), PROGRAM).unwrap();
    let output = stdout(plenty(dir.path(), &["--release", "p.plenty"]));
    let visible: String = output
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .flat_map(|line| [line, "\n"])
        .collect();
    assert_eq!(visible, EXPECTED);
}

/// Optimized code passes each operation code as an immediate, which the dump
/// shows; unoptimized code may load it from a register.
#[test]
fn unmanaged_environments_and_plain_payloads_skip_the_dispatcher() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), PROGRAM).unwrap();
    let args = [
        "--release",
        "--dump",
        "ops,asm",
        "--dump-std",
        "p.plenty",
        "-o",
        "out",
    ];
    stdout(plenty(dir.path(), &args));
    // A capture-less error mapper is copied, moved and dropped inline, as is
    // an environment holding only a scalar.
    for name in [
        "lift[i64] ",
        "std.error.map_error[i64, AllocError, Problem,",
        "scaled ",
        "scaled.<closure@18:10> ",
    ] {
        let calls = dispatches(dir.path(), name);
        assert!(calls.is_empty(), "{name}: {calls:?}");
    }
    // A `Result[Bytes, E]` result copies its record without relocation.
    let filled = dispatches(dir.path(), "filled ");
    assert!(!filled.contains(&COPY_PAYLOAD), "{filled:?}");
}
