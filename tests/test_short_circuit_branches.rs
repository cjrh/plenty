//! `and`, `or`, and `not` consumed by an `if`, `while`, `match`, or another
//! short circuit branch straight to the consumer's target instead of passing a
//! constant `bool` through a join block and testing it again (issue #377).
//! Operands still run in order, only when needed, with their traps and `?`.
mod support;

/// Each short-circuit function has a `_manual` twin written with nested `if`s
/// and `break`s; `main` prints the effects and results of one set, then the
/// other.
const PROGRAM: &str = r#"def say(label: str, value: bool) -> Result[bool, IoError]:
    print(label)?
    Ok(value)

def fails(label: str, value: bool) -> Result[bool, IoError | ParseError]:
    print(label)?
    if value:
        return Err(ParseError.Invalid)
    Ok(False)

def chain(x: bool, y: bool, z: bool) -> Result[bool, IoError]:
    if say("a", x)? or (say("b", y)? and not say("c", z)?):
        return Ok(True)
    Ok(False)

def chain_manual(x: bool, y: bool, z: bool) -> Result[bool, IoError]:
    if say("a", x)?:
        return Ok(True)
    if say("b", y)?:
        if say("c", z)?:
            return Ok(False)
        return Ok(True)
    Ok(False)

def stored(x: bool, y: bool, z: bool) -> Result[bool, IoError]:
    v = say("p", x)? and say("q", y)? or say("r", z)?
    if v and not x:
        print("v")?
    Ok(v)

def stored_manual(x: bool, y: bool, z: bool) -> Result[bool, IoError]:
    mut v = False
    if say("p", x)?:
        v = say("q", y)?
    if not v:
        v = say("r", z)?
    if v:
        if not x:
            print("v")?
    Ok(v)

def matched(x: bool, y: bool, z: bool) -> Result[i64, IoError]:
    match not (say("m", x)? or say("n", y)?) and not say("o", z)?:
        case True:
            Ok(1)
        case False:
            Ok(2)

def matched_manual(x: bool, y: bool, z: bool) -> Result[i64, IoError]:
    if say("m", x)?:
        return Ok(2)
    if say("n", y)?:
        return Ok(2)
    if say("o", z)?:
        return Ok(2)
    Ok(1)

def looped(limit: i64, stop: i64) -> i64:
    mut i = 0
    mut n = 0
    while i < limit and not (i % 7 == stop or n > 40):
        n += i
        i += 1
    n

def looped_manual(limit: i64, stop: i64) -> i64:
    mut i = 0
    mut n = 0
    while True:
        if i >= limit:
            break
        if i % 7 == stop:
            break
        if n > 40:
            break
        n += i
        i += 1
    n

def nonzero_quotient(d: i64) -> bool:
    d != 0 and 10 // d > 2

def zero_or_quotient(d: i64) -> bool:
    d == 0 or 10 // d > 2

def propagated(x: bool, y: bool) -> Result[bool, IoError | ParseError]:
    if say("s", x)? or fails("f", y)?:
        return Ok(True)
    Ok(False)

def main() -> Result[(), AllocError | IoError]:
    for manual in [False, True]?:
        print("manual", manual)?
        for bits in range(8):
            x = bits % 2 == 1
            y = (bits // 2) % 2 == 1
            z = bits // 4 == 1
            if manual:
                print(chain_manual(x, y, z)?, stored_manual(x, y, z)?, matched_manual(x, y, z)?)?
            else:
                print(chain(x, y, z)?, stored(x, y, z)?, matched(x, y, z)?)?
        for stop in range(8):
            if manual:
                print(looped_manual(12, stop))?
            else:
                print(looped(12, stop))?
    for d in [0, 1, 4, -1]?:
        print(nonzero_quotient(d), zero_or_quotient(d))?
    for bits in range(4):
        match propagated(bits % 2 == 1, bits // 2 == 1):
            case Ok(value):
                print("ok", value)?
            case Err(error):
                print("err", error)?
    Ok(())
"#;

/// `pick(False, 0)` runs the division its short circuit guards.
const TRAP: &str = r#"def pick(flag: bool, d: i64) -> bool:
    flag or 10 // d > 2

def main() -> Result[(), AllocError | IoError]:
    print(pick(True, 0))?
    if pick(False, 5) or not pick(False, 0):
        print("unreachable")?
    Ok(())
"#;

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

fn write(name: &str, source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(name), source).unwrap();
    dir
}

#[test]
fn short_circuits_match_their_hand_written_branches() {
    let dir = write("branches.plenty", PROGRAM);
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("branches.plenty");
        let out = stdout(plenty(dir.path(), &args));
        let (short, manual) = out
            .strip_prefix("manual False\n")
            .and_then(|rest| rest.split_once("manual True\n"))
            .unwrap_or_else(|| panic!("{mode:?}\n{out}"));
        let (manual, rest) = manual.split_at(short.len());
        assert_eq!(short, manual, "{mode:?}");
        // The first truth-table row: every operand runs.
        assert!(short.starts_with("a\nb\np\nr\nm\nn\no\nFalse False 1\n"));
        assert_eq!(
            rest,
            "False True\nTrue True\nFalse False\nFalse False\ns\nf\nok False\ns\nok True\ns\nf\nerr ParseError.Invalid\ns\nok True\n",
            "{mode:?}"
        );
    }
}

#[test]
fn skipped_operand_cannot_trap_and_evaluated_operand_does() {
    let dir = write("trap.plenty", TRAP);
    for mode in [&[][..], &["--release"][..]] {
        let mut args = mode.to_vec();
        args.push("trap.plenty");
        let output = plenty(dir.path(), &args);
        assert_eq!(output.status.code(), Some(1), "{mode:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), "True\n");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("error: trap.plenty:2:13: division by zero"),
            "{mode:?}: {stderr}"
        );
    }
}

/// The text of one function's section in a dump stage file.
fn section(dir: &std::path::Path, stage: &str, name: &str) -> String {
    let text = std::fs::read_to_string(dir.join("out").join(stage)).unwrap();
    let start = text
        .find(&format!("==== {name} "))
        .unwrap_or_else(|| panic!("no `{name}` in {stage}"));
    let rest = &text[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

/// `bool` block parameters and branches in a function's optimized CLIF.
fn shape(dir: &std::path::Path, name: &str) -> (usize, usize) {
    let body = section(dir, "clif-opt.txt", name);
    let params = body.matches(": i8)").count() + body.matches(": i8,").count();
    (params, body.matches(" brif ").count())
}

#[test]
fn consumed_short_circuits_branch_like_nested_ifs() {
    let dir = write("branches.plenty", PROGRAM);
    stdout(plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif-opt",
            "branches.plenty",
            "-o",
            "out",
        ],
    ));
    // The remaining `bool` parameters carry `?`-propagated call results,
    // which the nested-`if` versions have too.
    for name in ["chain", "matched", "looped"] {
        assert_eq!(
            shape(dir.path(), name),
            shape(dir.path(), &format!("{name}_manual")),
            "{name}:\n{}",
            section(dir.path(), "clif-opt.txt", name)
        );
    }
    assert_eq!(shape(dir.path(), "looped").0, 0);
}
