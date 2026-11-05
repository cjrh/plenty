//! Optimized builds lower small, non-recursive functions at their call sites
//! (issue #722). An inlined function keeps its own body only while something
//! other than an inlined call refers to it. Only optimized builds inline,
//! so these tests run the command-line compiler both ways.
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

/// Each inlined function is also reached another way: through a generator,
/// a function value, a protocol table, a recursive caller, or a `?` failure
/// and a trap inside it. The bodies select trapping arithmetic with
/// `unchecked:` so the division trap stays observable.
const PROGRAM: &str = r#"protocol Sized:
    def size(self) -> i64:
        pass

class Square:
    side: i64

    def size(self) -> i64:
        unchecked:
            self.side * self.side

def twice(x: i64) -> i64:
    unchecked:
        x + x

def numbers(stop: i64) -> Generator[i64]:
    mut n = 0
    while n < stop:
        yield twice(n)
        unchecked:
            n += 1

def apply(f: Callable[[i64], i64], x: i64) -> i64:
    f(x)

def checked(xs: &[i64], i: i64) -> Result[i64, AllocError]:
    if i < 0:
        return Err(AllocError.CapacityOverflow)
    unchecked:
        Ok(xs[i] + twice(i))

def depth(n: i64) -> i64:
    if n == 0:
        return 0
    unchecked:
        depth(n - 1) + 1

def ratio(a: i64, b: i64) -> i64:
    mut total = 0
    for i in range(3):
        unchecked:
            total += a // (b - i)
    total

def main() -> Result[(), AllocError | IoError]:
    xs = [5, 6, 7]?
    print(list(numbers(3))?, apply(twice, 4), twice(5))?
    s = Square(3)
    boxed = Box[Sized](Square(4))?
    print(s.size(), boxed.size(), depth(3))?
    print(checked(&xs, 1)?)?
    match checked(&xs, -1):
        case Ok(v):
            print("unexpected", v)?
        case Err(e):
            print("failed", str.repr(e)?)?
    print(ratio(12, 4))?
    print(ratio(1, 2))?
    Ok(())
"#;

const EXPECTED: &str = "[0, 2, 4] 8 10\n9 16 3\n8\nfailed AllocError.CapacityOverflow\n13\n";

#[test]
fn inlined_functions_keep_results_failures_and_traps() {
    let debug = run(PROGRAM, &["--fast-compile"]);
    let release = run(PROGRAM, &["--release"]);
    assert_eq!(debug.1, EXPECTED, "{}", debug.2);
    assert_eq!(debug.0, Some(1));
    assert!(
        debug.2.contains("p.plenty:42:22: division by zero"),
        "{}",
        debug.2
    );
    assert_eq!(release, debug);
}

/// A function called once and a small one called from several places are
/// lowered in their callers; a large one with several callers and a
/// recursive one stay calls. Bodies use trapping arithmetic so the shapes
/// match the inliner's size limits without overflow results.
const SHAPES: &str = r#"def once(xs: &mut [i64], n: i64) -> i64:
    unchecked:
        mut lo = 0
        mut hi = n - 1
        mut swaps = 0
        while lo < hi:
            t = xs[lo]
            xs[lo] = xs[hi]
            xs[hi] = t
            lo += 1
            hi -= 1
            swaps += 1
        swaps

def small(x: i64) -> i64:
    unchecked:
        x * 3

def shared(xs: &[i64]) -> i64:
    unchecked:
        mut total = 0
        for i in range(len(xs)):
            total += xs[i] * xs[i] + small(i)
        if total > 1000:
            total = total // 2
        total

def depth(n: i64) -> i64:
    if n == 0:
        return 0
    unchecked:
        depth(n - 1) + 1

def caller(xs: &mut [i64]) -> i64:
    unchecked:
        n = len(xs)
        swaps = once(xs, n)
        swaps + small(2) + shared(xs) + shared(xs) + depth(2)

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4]?
    print(caller(&mut xs), small(1), xs)?
    Ok(())
"#;

#[test]
fn small_and_single_site_functions_are_lowered_in_their_callers() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), SHAPES).unwrap();
    let args = ["--release", "--dump", "asm", "p.plenty", "-o", "out"];
    let output = plenty(dir.path(), &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ran = plenty(dir.path(), &["--release", "p.plenty"]);
    assert_eq!(
        String::from_utf8(ran.stdout).unwrap(),
        "106 3 [4, 3, 2, 1]\n"
    );
    let text = std::fs::read_to_string(dir.path().join("out").join("asm.txt")).unwrap();
    let start = text.find("==== caller ").expect("caller dumped");
    let rest = &text[start + 5..];
    let section = &rest[..rest.find("\n==== ").unwrap_or(rest.len())];
    let calls: Vec<&str> = section
        .lines()
        .filter_map(|line| line.rsplit_once("# __plenty_fn_"))
        .map(|(_, callee)| callee.trim())
        .collect();
    assert_eq!(calls, ["shared", "shared", "depth"], "{section}");
}

/// A function inlined at its only call site keeps its symbol in the object,
/// where C code relinking the object can still reach it. Its body is a trap
/// instruction, not empty code that would run into the next function.
#[cfg(unix)]
#[test]
fn unreferenced_inlined_functions_trap_if_called() {
    use object::{Object, ObjectSection, ObjectSymbol};
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    // The loop keeps call substitution, which keeps the callee's body, from
    // taking `helper` first.
    let source = "def helper(n: i64) -> i64:\n    mut total = 0\n    for i in range(n):\n        \
                  unchecked:\n            total += i\n    total\n\n\
                  def main() -> Result[(), IoError]:\n    print(helper(5))?\n    Ok(())\n";
    std::fs::write(dir.path().join("p.plenty"), source).unwrap();
    // The linker receives the program object first; keep a copy of it.
    let linker = dir.path().join("capture.sh");
    std::fs::write(
        &linker,
        "#!/bin/sh\ncp \"$1\" \"$(dirname \"$0\")/p.o\"\nexec cc \"$@\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&linker, std::fs::Permissions::from_mode(0o755)).unwrap();
    let args = ["--release", "--compile", "--no-strip", "--linker"];
    let mut all = args.map(String::from).to_vec();
    all.extend([
        linker.display().to_string(),
        "p.plenty".into(),
        "-o".into(),
        "p".into(),
    ]);
    let all: Vec<&str> = all.iter().map(String::as_str).collect();
    let output = plenty(dir.path(), &all);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ran = std::process::Command::new(dir.path().join("p"))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(ran.stdout).unwrap(), "10\n");

    let bytes = std::fs::read(dir.path().join("p.o")).unwrap();
    let file = object::File::parse(&*bytes).unwrap();
    let symbol = file
        .symbols()
        .find(|symbol| {
            symbol
                .name()
                .is_ok_and(|name| name.ends_with("__plenty_fn_helper"))
        })
        .expect("helper keeps its symbol");
    let section = file
        .section_by_index(symbol.section_index().unwrap())
        .unwrap();
    let offset = (symbol.address() - section.address()) as usize;
    // Allow the frame setup before the trap: on AArch64 the two prologue
    // instructions alone occupy eight bytes, followed by a four-byte UDF.
    let (trap, max_size): (&[u8], usize) = match file.architecture() {
        object::Architecture::X86_64 => (&[0x0f, 0x0b], 8),
        object::Architecture::Aarch64 => (&0xc11f_u32.to_le_bytes(), 12),
        other => panic!("unexpected architecture {other:?}"),
    };
    let code = &section.data().unwrap()[offset..];
    let code = &code[..code.len().min(max_size)];
    assert!(code.windows(trap.len()).any(|w| w == trap), "{code:x?}");
    if symbol.size() != 0 {
        assert!(symbol.size() <= max_size as u64, "{}", symbol.size());
    }
}
