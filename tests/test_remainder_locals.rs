//! A local every loop write leaves as a remainder by a positive constant is
//! carried in a constant range, so arithmetic on it that cannot overflow has
//! no overflow check and its remainder no sign correction (issue #940).
use std::path::Path;
use std::process::Command;

mod support;

fn plenty(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

/// The `--release` CLIF of function `name` in `source`.
fn clif(source: &str, name: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif-opt",
            "main.plenty",
            "-o",
            "out",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dump = std::fs::read_to_string(dir.path().join("out/clif-opt.txt")).unwrap();
    let start = dump
        .find(&format!("==== {name} "))
        .unwrap_or_else(|| panic!("no section {name} in:\n{dump}"));
    let rest = &dump[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

fn checks(body: &str) -> usize {
    body.matches("smul_overflow").count() + body.matches("sadd_overflow").count()
}

const SOURCE: &str = "\
const IM: i64 = 139968
const IA: i64 = 3877
const IC: i64 = 29573

type Checked = Result[i64, OverflowError[i64] | ZeroDivisionError]


def carried(seed: i64, out: &mut [i64]) -> Checked:
    mut s = seed % IM
    for i in range(len(out)):
        s = (s * IA + IC) % IM
        out[i] = s
    Ok(s)


def arms(seed: i64, out: &mut [i64]) -> Checked:
    mut s = seed % IM
    for i in range(len(out)):
        if i % 2 == 0:
            s = (s * IA + IC) % IM
        else:
            s = (s * 3 + 1) % 1000
        out[i] = s
    Ok(s)


def inner(seed: i64, out: &mut [i64], rounds: i64) -> Checked:
    mut s = seed % IM
    for _ in range(rounds):
        for i in range(len(out)):
            s = (s * IA + IC) % IM
            out[i] = s
    Ok(s)


def condition(seed: i64) -> Checked:
    mut s = seed % IM
    mut steps = 0
    while s * IA + IC != 29573:
        s = (s * IA + IC) % IM
        steps += 1
    Ok(steps)


def other_write(seed: i64, out: &mut [i64]) -> Checked:
    mut s = seed % IM
    for i in range(len(out)):
        s = (s * IA + IC) % IM
        if s == 7:
            s = s + i
        out[i] = s
    Ok(s)


def unknown_entry(seed: i64, out: &mut [i64]) -> Checked:
    mut s = seed
    for i in range(len(out)):
        s = (s * IA + IC) % IM
        out[i] = s
    Ok(s)


def variable_divisor(seed: i64, out: &mut [i64], m: i64) -> Checked:
    mut s = seed % IM
    for i in range(len(out)):
        s = (s * IA + IC) % m
        out[i] = s
    Ok(s)


def bump(s: &mut i64) -> ():
    *s = 9223372036854775807


def borrowed(seed: i64, out: &mut [i64]) -> Checked:
    mut s = seed % IM
    for i in range(len(out)):
        s = (s * IA + IC) % IM
        if i == 2:
            bump(&mut s)
        out[i] = s
    Ok(s)
";

/// Each read of the carried local is in `0..=IM - 1`: the multiply, the add
/// and the floored remainder's sign correction need no check. The local is
/// read in a branch, in a nested loop and in a `while` condition.
#[test]
fn remainder_writes_bound_the_local() {
    let source = format!("{SOURCE}\n\ndef main() -> ():\n    ()\n");
    for function in ["carried", "arms", "inner"] {
        let body = clif(&source, function);
        assert_eq!(checks(&body), 0, "{function}:\n{body}");
    }
    // The step counter keeps its check.
    let body = clif(&source, "condition");
    assert_eq!(checks(&body), 1, "{body}");
    // Only the entry remainder, of a parameter of either sign, is corrected.
    let body = clif(&source, "carried");
    assert_eq!(body.matches("sshr").count(), 2, "{body}");
}

/// One write that is not a remainder, an entry value with no bound, a
/// divisor that is not a constant or an exclusive borrow leaves the local
/// without a range, so the checks stay.
#[test]
fn any_other_write_keeps_the_checks() {
    let source = format!("{SOURCE}\n\ndef main() -> ():\n    ()\n");
    for function in [
        "other_write",
        "unknown_entry",
        "variable_divisor",
        "borrowed",
    ] {
        let body = clif(&source, function);
        assert!(body.contains("smul_overflow"), "{function}:\n{body}");
        assert!(body.contains("sadd_overflow"), "{function}:\n{body}");
    }
}

#[test]
fn results_and_overflow_reports_are_unchanged() {
    let main = "
def show(r: Checked) -> Result[(), IoError]:
    match r:
        case Ok(v):
            print(v)?
        case Err(_):
            print(\"error\")?
    Ok(())


def main() -> Result[(), AllocError | IoError]:
    mut out = [0, 0, 0, 0]?
    show(carried(-42, &mut out))?
    print(out)?
    show(arms(42, &mut out))?
    show(inner(42, &mut out, 3))?
    show(condition(1))?
    show(other_write(42, &mut out))?
    show(unknown_entry(9223372036854775807, &mut out))?
    show(variable_divisor(42, &mut out, 0))?
    show(borrowed(42, &mut out))?
    Ok(())
";
    let source = format!("{SOURCE}{main}");
    for flags in [&[][..], &["--release"][..]] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.plenty"), &source).unwrap();
        let mut args = flags.to_vec();
        args.push("main.plenty");
        let output = plenty(dir.path(), &args);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "94274\n[6707, 138532, 60921, 94274]\n242\n96366\n50299\n111062\nerror\nerror\nerror\n",
            "{flags:?} {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
