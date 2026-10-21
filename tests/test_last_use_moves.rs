//! A copyable owned local read for the last time is moved rather than copied
//! (issue #388): a `match` on an owned parameter and a payload binding read
//! once no longer count a new owner and release the old one.
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

/// The dump section of the function named `name` in `file`.
fn section(dir: &std::path::Path, file: &str, name: &str) -> String {
    let text = std::fs::read_to_string(dir.join("out").join(file)).unwrap();
    let start = text
        .find(&format!("==== {name} "))
        .unwrap_or_else(|| panic!("{name} not dumped"));
    let rest = &text[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

// `repeated` reads `word` in a loop and `kept` uses its scrutinee after the
// match, so both keep copies; `lent` reads a borrowed local; `guarded` checks
// that a destructor still runs at return.
const PROGRAM: &str = r#"enum Problem:
    Wrapped(AllocError)
    Empty

class Noisy:
    name: str

    def __del__(self) -> ():
        print("drop", self.name).unwrap()

def passed(result: Result[str, AllocError]) -> Result[str, Problem]:
    match result:
        case Ok(value):
            Ok(value)
        case Err(cause):
            Err(Problem.Wrapped(cause))

def repeated(word: str, n: i64) -> Result[str, AllocError]:
    mut out = ""
    for i in range(n):
        out = (out + word)?
    Ok(out)

def looped(word: str) -> Result[str, AllocError]:
    mut out = ""
    mut i = 0
    while True:
        i += 1
        if i == 2:
            continue
        elif i > 4:
            break
        out = (out + word)?
    Ok((out + word)?)

def chosen(flag: bool, word: str) -> Result[str, AllocError]:
    pair = (word, 3)
    if flag:
        first, n = pair
        return Ok(first)
    text, k = pair
    if k > 5:
        return Ok(text)
    print(text).unwrap()
    Ok((word + "!")?)

def lent(word: str) -> Result[(), AllocError | IoError]:
    r = &word
    t = word
    print(t)?
    print(*r)?
    Ok(())

def guarded(word: str) -> Result[str, AllocError | IoError]:
    noisy = Noisy(name=word)
    copied = word
    print("before return")?
    Ok(copied)

def captured(word: str) -> Result[str, AllocError]:
    f = def once [word](suffix: str) -> Result[str, AllocError]:
        word + suffix
    f("?")

def propagated(word: str, fail: bool) -> Result[str, Problem]:
    kept = word
    if fail:
        return Err(Problem.Empty)
    Ok(kept)

def kept(result: Result[str, AllocError]) -> Result[str, AllocError]:
    match result:
        case Ok(value):
            print(value).unwrap()
        case Err(cause):
            pass
    result

def main() -> Result[(), AllocError | IoError | Problem]:
    mut n = 0
    for i in range(1000):
        text = passed(("a" + str.from(i)?))?
        n += len(text)
    print(n)?
    print(repeated("ab", 3)?)?
    print(looped("x")?)?
    print(chosen(True, "yes")?)?
    print(chosen(False, "no")?)?
    lent("lent")?
    print(guarded("g")?)?
    print(captured("c")?)?
    print(propagated("p", False)?)?
    match propagated("q", True):
        case Ok(text):
            print(text)?
        case Err(problem):
            print("failed")?
    print(kept(Ok("k"))?)?
    print("__test_small_live_heap__")?
    Ok(())
"#;

const EXPECTED: &str =
    "3890\nababab\nxxxx\nyes\nno\nno!\nlent\nlent\nbefore return\ndrop g\ng\nc?\np\nfailed\nk\nk\n";

#[test]
fn last_use_moves_keep_behaviour() {
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

#[test]
fn match_on_an_owned_parameter_moves_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), PROGRAM).unwrap();
    let args = ["--release", "--dump", "ops,asm", "p.plenty", "-o", "out"];
    stdout(plenty(dir.path(), &args));
    // The parameter and the payload binding are moved, and reading the
    // scrutinee's tag counts no copy: no retain (operation 26) remains.
    let ops = section(dir.path(), "ops.txt", "passed");
    assert!(ops.contains("MoveLocal(0)"), "{ops}");
    assert!(ops.contains("MoveLocal(2)"), "{ops}");
    assert!(!ops.contains("LoadLocal(0)"), "{ops}");
    let asm = section(dir.path(), "asm.txt", "passed");
    assert!(!asm.contains("mov     $0x1A,%edi"), "{asm}");
    // A scrutinee read after the match stays a copy.
    let kept = section(dir.path(), "ops.txt", "kept");
    assert!(kept.contains("LoadLocal(0)"), "{kept}");
}
