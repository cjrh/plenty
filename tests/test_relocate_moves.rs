//! Every move of every top-level declaration in a few fixture packages
//! (issue #822). Each move is either refused, without edits and with the
//! same message when asked again, or produces a package whose programs and
//! tests print exactly what the original printed, apart from the printed
//! type names that the move reports. Moving an applied move back gives the
//! original text.
use plenty::ide::relocate;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const TOOLS: &str = "manifest-version = 1\n\n[package]\nname = \"tools\"\nversion = \"0.1.0\"\n";

/// Shadowing parameters and locals, a method named like a module
/// function, a decorator, aliases, `from` imports, a std import, and a
/// multi-line string holding `#` and `import` lines.
const SHADOWING: &[(&str, &str)] = &[
    ("plenty.toml", TOOLS),
    (
        "src/geometry.plenty",
        "\
import std.math as m

pub const SCALE: i64 = 10

# A point on the grid.
@copy
pub class Point:
    pub x: i64
    pub y: i64

    pub def scale(self) -> Result[i64, OverflowError[i64]]:
        Ok(self.x * SCALE)

pub def scale(p: &Point) -> Result[i64, OverflowError[i64]]:
    Ok(p.y * SCALE)

# The parameter shadows the module's constant.
def offset(SCALE: i64) -> i64:
    SCALE

pub def shifted(x: i64) -> Result[i64, OverflowError[i64]]:
    scale = 3
    Ok(offset(x) + scale)

pub def norm(p: &Point) -> f64:
    \"\"\"The distance from the origin's column.\"\"\"
    m.fabs(f64(p.x))
",
    ),
    (
        "src/units.plenty",
        "\
pub enum Unit:
    Metre
    Foot

pub def label(u: &Unit) -> str:
    match u:
        case Unit.Metre:
            \"m\"
        case Unit.Foot:
            \"ft\"
",
    ),
    (
        "src/report.plenty",
        "\
import geometry as g
from geometry import Point as P, SCALE
from units import Unit, label

pub const BANNER: str = \"\"\"Report
# not a comment
import geometry
end\"\"\"

pub def describe(p: &P, u: &Unit) -> Result[str, AllocError | OverflowError[i64]]:
    scaled = g.scale(p)?
    f\"{p.x},{p.y} by {SCALE} is {scaled} {label(u)}\"
",
    ),
    (
        "src/main.plenty",
        "\
import geometry
import report
from units import Unit

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    p = geometry.Point(x=1, y=2)
    print(p)?
    print(p.scale()?, geometry.scale(&p)?, geometry.shifted(4)?, geometry.norm(&p))?
    print(report.describe(&p, &Unit.Foot)?)?
    print(report.BANNER)?
    print(Unit.Metre)?
    Ok(())
",
    ),
];

/// A library, a nested module with CRLF line endings, a multi-line string
/// and a decorator, an included file, and tests.
const NESTED: &[(&str, &str)] = &[
    ("plenty.toml", TOOLS),
    ("src/lib.plenty", "pub def version() -> i64:\n    2\n"),
    (
        "src/text/strings.plenty",
        "pub const GREETING: str = include_str(\"greeting.txt\")\n\npub def greet() -> str:\n    GREETING\n",
    ),
    ("src/text/greeting.txt", "hello from a file"),
    (
        "src/text/format.plenty",
        "from text.strings import greet\r\n\r\npub const POEM: str = \"\"\"one\r\ntwo\r\n\"\"\"\r\n\r\n# Hidden on display.\r\n@redacted\r\npub class Secret:\r\n    pub code: i64\r\n\r\npub def wrap(s: &Secret) -> Result[str, AllocError]:\r\n    f\"[{s.code}] {greet()}\"\r\n",
    ),
    (
        "src/main.plenty",
        "\
import lib
import text.strings as strings
from text.format import POEM, Secret, wrap

def main() -> Result[(), AllocError | IoError]:
    print(lib.version())?
    print(strings.greet(), POEM.byte_len())?
    s = Secret(code=7)
    print(s, wrap(&s)?)?
    Ok(())
",
    ),
    (
        "tests/test_text.plenty",
        "\
import std.test as t
import tools
from tools.text.format import Secret, wrap
import tools.text.strings

def test_wrap() -> Result[(), AllocError | t.TestError]:
    t.equal(&wrap(&Secret(code=1))?, &\"[1] hello from a file\")?
    t.equal(&tools.text.strings.GREETING, &\"hello from a file\")?
    t.equal(&tools.version(), &2)?
    Ok(())
",
    ),
];

/// A type alias, a protocol, two classes with a method of the same name,
/// names bound by a comprehension and a case pattern, `if const` blocks,
/// a `.plentyi` module, non-ASCII text before a rewritten name, and a test
/// helper module under `tests/`.
const VARIETY: &[(&str, &str)] = &[
    ("plenty.toml", TOOLS),
    (
        "src/lib.plenty",
        "\
pub def version() -> i64:
    3
",
    ),
    (
        "src/shapes.plenty",
        "\
pub type Side = i64

# Two classes with a method of the same name.
pub class Square:
    pub side: Side

    pub def area(self) -> Result[i64, OverflowError[i64]]:
        Ok(self.side * self.side)

pub class Strip:
    pub width: Side
    pub length: Side

    pub def area(self) -> Result[i64, OverflowError[i64]]:
        Ok(self.width * self.length)

pub protocol Measured:
    def area(self) -> Result[i64, OverflowError[i64]]:
        pass

pub def doubled[T: Measured](item: &T) -> Result[i64, OverflowError[i64]]:
    Ok(item.area()? * 2)
",
    ),
    (
        "src/limits.plenty",
        "\
pub const LIMIT: i64 = 3
",
    ),
    (
        "src/calc.plenty",
        "\
from limits import LIMIT

# An import in an arm, which no move rewrites.
if const target.os == \"windows\":
    import std.path

# The comprehension binds its own `LIMIT`.
pub def firsts(n: i64) -> Result[list[i64], AllocError]:
    [LIMIT for LIMIT in range(n)]

# So does the case pattern.
pub def clamp(value: Option[i64]) -> i64:
    match value:
        case Some(LIMIT):
            LIMIT
        case Nothing:
            0

pub def cap(x: i64) -> i64:
    LIMIT if x > LIMIT else x

pub def platform() -> str:
    if const target.os == \"macos\":
        name = \"mac\"
    else:
        name = \"other\"
    name
",
    ),
    (
        "src/native.plentyi",
        "\
extern def getpid_raw() -> i32 = \"getpid\"

pub def has_pid() -> bool:
    getpid_raw() > 0

# A wrapper that does not use the extern, so it can move.
pub def pid_label() -> i64:
    7
",
    ),
    (
        "src/main.plenty",
        "\
import calc
import limits
import native
import shapes

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    s = shapes.Square(side=3)
    print(s.area()?, shapes.Strip(width=2, length=5).area()?, shapes.doubled(&s)?)?
    side: shapes.Side = 4
    print(side)?
    print(calc.firsts(3)?, calc.clamp(Some(7)), calc.platform())?
    print(\"é ü\", calc.cap(5))?
    if const target.os == \"windows\":
        print(\"windows\")?
    print(limits.LIMIT)?
    print(native.pid_label(), native.has_pid())?
    Ok(())
",
    ),
    (
        "tests/support/build.plenty",
        "\
from tools.shapes import Square

pub def unit_square() -> Square:
    Square(side=1)
",
    ),
    (
        "tests/test_shapes.plenty",
        "\
import std.test as t
import support.build
from tools.shapes import Strip, doubled

def test_area() -> Result[(), AllocError | OverflowError[i64] | t.TestError]:
    t.equal(&support.build.unit_square().area()?, &1)?
    t.equal(&doubled(&Strip(width=2, length=3))?, &12)?
    Ok(())
",
    ),
];

/// A library package with tests and a workspace member that depends on it.
const WORKSPACE: &[(&str, &str)] = &[
    (
        "plenty.toml",
        "manifest-version = 1\n\n[workspace]\nmembers = [\"tools\", \"report\", \"app\"]\n",
    ),
    ("tools/plenty.toml", TOOLS),
    (
        "tools/src/lib.plenty",
        "from shapes import Point\n\npub def origin() -> Point:\n    Point(x=0)\n",
    ),
    (
        "tools/src/shapes.plenty",
        "\
import colors

@copy
pub class Point:
    pub x: i64

pub def describe(p: &Point) -> Result[str, AllocError]:
    f\"{p.x} in {colors.name(&colors.Color.Red)}\"
",
    ),
    (
        "tools/src/colors.plenty",
        "\
pub enum Color:
    Red
    Blue

pub def name(c: &Color) -> str:
    match c:
        case Color.Red:
            \"red\"
        case Color.Blue:
            \"blue\"
",
    ),
    (
        "tools/tests/test_shapes.plenty",
        "\
import std.test as t
from tools.shapes import Point, describe

def test_describe() -> Result[(), AllocError | t.TestError]:
    t.equal(&describe(&Point(x=3))?, &\"3 in red\")?
    Ok(())
",
    ),
    (
        "app/plenty.toml",
        "manifest-version = 1\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\ntools = { path = \"../tools\" }\nreport = { path = \"../report\" }\n",
    ),
    (
        "report/plenty.toml",
        "manifest-version = 1\n\n[package]\nname = \"report\"\nversion = \"0.1.0\"\n\n[dependencies]\nkit = { path = \"../tools\" }\n",
    ),
    (
        "report/src/lib.plenty",
        "\
# `tools` under the alias `kit`, and a module of it under `s`.
import kit.shapes as s
from kit.colors import Color, name

pub def summary() -> Result[str, AllocError]:
    p = s.Point(x=9)
    f\"{name(&Color.Blue)}: {s.describe(&p)?}\"
",
    ),
    (
        "app/src/main.plenty",
        "\
import report
import tools
import tools.colors as c
from tools.shapes import Point, describe

def main() -> Result[(), AllocError | IoError]:
    p = Point(x=5)
    print(p, describe(&p)?, tools.origin())?
    print(c.Color.Blue, c.name(&c.Color.Blue))?
    print(report.summary()?)?
    Ok(())
",
    ),
];

type Files = BTreeMap<String, String>;

fn write(dir: &Path, files: &Files) {
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

/// `files` with the target names `\"windows\"` and `\"linux\"` exchanged in
/// every source file, so `plenty check` on this machine checks the arms it
/// would otherwise skip.
fn swapped(files: &Files) -> Files {
    files
        .iter()
        .map(|(path, text)| {
            let text = if path.ends_with(".plenty") || path.ends_with(".plentyi") {
                text.replace("\"windows\"", "\u{0}")
                    .replace("\"linux\"", "\"windows\"")
                    .replace('\u{0}', "\"linux\"")
            } else {
                text.clone()
            };
            (path.clone(), text)
        })
        .collect()
}

/// Whether `plenty check` accepts `files`.
fn checks(files: &Files) -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), files);
    warnings(dir.path()).map(|_| ())
}

/// What `plenty run` and, with tests, `plenty test` print in `dir`.
fn behaviour(dir: &Path, tests: bool) -> Result<String, String> {
    let mut out = String::new();
    let commands: &[&str] = if tests { &["run", "test"] } else { &["run"] };
    for command in commands {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args([command, "--fast-compile"])
            .current_dir(dir)
            .output()
            .unwrap();
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        out.push_str(&String::from_utf8(output.stdout).unwrap());
    }
    Ok(out)
}

/// The warnings `plenty check` reports in `dir`, without their locations,
/// which a move changes. The warning for a file no target imports is left
/// out: a move can leave such a file behind, as the reference documents.
fn warnings(dir: &Path) -> Result<Vec<String>, String> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("check")
        .current_dir(dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(stderr);
    }
    let mut out: Vec<String> = stderr
        .lines()
        .filter_map(|line| line.strip_prefix("warning: "))
        .filter(|line| !line.contains("is not imported by any target"))
        .map(|line| {
            // Drop a leading `path:line:column: `.
            let mut parts = line.splitn(4, ':');
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(_), Some(l), Some(c), Some(rest))
                    if l.parse::<u32>().is_ok() && c.parse::<u32>().is_ok() =>
                {
                    rest.trim_start().to_owned()
                }
                _ => line.to_owned(),
            }
        })
        .collect();
    out.sort();
    Ok(out)
}

/// `text` with each printed type name `old` replaced by `new`. A name
/// follows a non-identifier character or the `::` after a dependency's
/// package name, and ends before one that cannot continue it.
fn rename_printed(text: &str, old: &str, new: &str) -> String {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(old) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + old.len()..].chars().next();
        out.push_str(&rest[..at]);
        if before.is_none_or(|c| !ident(c) && c != '.') && after.is_none_or(|c| !ident(c)) {
            out.push_str(new);
        } else {
            out.push_str(old);
        }
        rest = &rest[at + old.len()..];
    }
    out.push_str(rest);
    out
}

/// The output expected after a move: the original's, with the printed
/// type names the move reports renamed.
fn expected_output(original: &str, api_changes: &[String]) -> String {
    let mut out = original.to_owned();
    for change in api_changes {
        if let Some(rest) = change.strip_prefix("changes the printed type name `") {
            let (old, rest) = rest.split_once("` to `").unwrap();
            let new = rest.strip_suffix('`').unwrap();
            out = rename_printed(&out, old, new);
        }
    }
    out
}

/// Every top-level declaration of the package's modules that a move can
/// start from: `(file, first line, name, destination files)`.
fn declarations(
    root: &Path,
    package: &str,
    files: &Files,
) -> Vec<(String, u32, String, Vec<String>)> {
    let src = format!("{package}src/");
    let mut found = vec![];
    for (path, text) in files {
        if !path.starts_with(&src) || !(path.ends_with(".plenty") || path.ends_with(".plentyi")) {
            continue;
        }
        let mut seen = vec![];
        for line in 1..=text.lines().count() as u32 {
            let Some(candidate) = relocate::candidates(&root.join(path), text, line) else {
                continue;
            };
            if seen.contains(&candidate.name) {
                continue;
            }
            seen.push(candidate.name.clone());
            let destinations = candidate
                .destinations
                .iter()
                .map(|(_, f)| f.strip_prefix(root).unwrap().display().to_string())
                .collect();
            found.push((path.clone(), line, candidate.name, destinations));
        }
    }
    found
}

/// The first line (from 1) of the declaration `name` in `text`.
fn line_of(path: &Path, text: &str, name: &str) -> Option<u32> {
    (1..=text.lines().count() as u32)
        .find(|&line| relocate::candidates(path, text, line).is_some_and(|c| c.name == name))
}

/// Plan a move against `overlay` (by path relative to `root`) and apply
/// its edits to `files`.
fn plan(
    root: &Path,
    files: &Files,
    file: &str,
    line: u32,
    destination: &str,
) -> Result<(Files, Vec<String>), String> {
    let overlay: HashMap<PathBuf, String> = files
        .iter()
        .map(|(path, text)| (root.join(path), text.clone()))
        .collect();
    let plan = relocate::plan(&root.join(file), line, &root.join(destination), &overlay)?;
    let mut after = files.clone();
    for edits in &plan.files {
        let path = edits.path.strip_prefix(root).unwrap().display().to_string();
        let text = after.get_mut(&path).unwrap();
        *text = relocate::apply(text, &edits.edits).unwrap();
    }
    Ok((after, plan.api_changes))
}

/// The non-blank lines that differ between `before` and `after`, as a
/// minimal diff: `-` for a removed line and `+` for an added one.
fn line_diff(before: &str, after: &str) -> Vec<String> {
    let a: Vec<&str> = before.lines().filter(|l| !l.trim().is_empty()).collect();
    let b: Vec<&str> = after.lines().filter(|l| !l.trim().is_empty()).collect();
    // Longest common subsequence lengths of each pair of suffixes.
    let mut common = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            common[i][j] = if a[i] == b[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, vec![]);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j == b.len() || (i < a.len() && common[i + 1][j] >= common[i][j + 1]) {
            out.push(format!("- {}", a[i]));
            i += 1;
        } else {
            out.push(format!("+ {}", b[j]));
            j += 1;
        }
    }
    out
}

/// How a round trip that is not exact differs from the original, file by
/// file.
fn difference(original: &Files, back: &Files) -> String {
    let mut out = String::new();
    for (path, text) in original {
        let diff = line_diff(text, &back[path]);
        if !diff.is_empty() {
            out.push_str(&format!("  {path}\n"));
            for line in diff {
                out.push_str(&format!("    {line}\n"));
            }
        }
    }
    out
}

/// Every file below `dir` except the build output in `dir/target`, by
/// relative path.
fn listing(dir: &Path) -> Files {
    fn walk(root: &Path, dir: &Path, out: &mut Files) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path != root.join("target") {
                    walk(root, &path, out);
                }
            } else {
                let relative = path.strip_prefix(root).unwrap().display().to_string();
                let bytes = std::fs::read(&path).unwrap();
                out.insert(relative, String::from_utf8_lossy(&bytes).into_owned());
            }
        }
    }
    let mut out = Files::new();
    walk(dir, dir, &mut out);
    out
}

fn changes(before: &Files, after: &Files) -> String {
    after
        .iter()
        .filter(|(path, text)| before[*path] != **text)
        .map(|(path, text)| format!("--- {path} was\n{}--- now\n{text}", before[path]))
        .collect()
}

/// Whether moving a declaration back restores every file exactly. When it
/// does not, a second round trip must change nothing; the caller also runs
/// the text moved back.
fn round_trip(
    root: &Path,
    original: &Files,
    back: &Files,
    file: &str,
    destination: &str,
    name: &str,
) -> Result<bool, String> {
    if back == original {
        return Ok(true);
    }
    let line = line_of(&root.join(file), &back[file], name).ok_or("lost the declaration")?;
    let (there, _) = plan(root, back, file, line, destination)
        .map_err(|e| format!("then refused the move again: {e}"))?;
    let line = line_of(&root.join(destination), &there[destination], name)
        .ok_or("lost the declaration")?;
    let (again, _) = plan(root, &there, destination, line, file)
        .map_err(|e| format!("then refused the move back again: {e}"))?;
    if again != *back {
        return Err(format!("then changed again\n{}", changes(back, &again)));
    }
    Ok(false)
}

#[derive(Debug, Default, PartialEq)]
struct Counts {
    tried: usize,
    applied: usize,
    refused: usize,
    /// Applied moves whose reverse move is also accepted.
    round_trips: usize,
    /// Round trips that give back the original text exactly.
    exact_round_trips: usize,
    /// Applied moves, and inexact texts moved back, checked again with the
    /// other target's arms selected.
    swapped_checks: usize,
}

struct Applied {
    label: String,
    files: Files,
    api_changes: Vec<String>,
}

/// What trying every move found.
struct Report {
    counts: Counts,
    /// Each refusal, with paths relative to the fixture's root.
    refusals: String,
    /// Each inexact round trip and its difference from the original.
    inexact: String,
    failures: Vec<String>,
}

fn every_move(fixture: &[(&str, &str)], package: &str, tests: bool) -> Report {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let original: Files = fixture
        .iter()
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .collect();
    write(&root, &original);
    let output = behaviour(&root, tests).unwrap();
    let warned = warnings(&root).unwrap();
    // A fixture with `if const` blocks whose other arms check is checked
    // again after each move with those arms selected.
    let swap = original.values().any(|text| text.contains("if const"))
        && checks(&swapped(&original)).is_ok();
    let on_disk = listing(&root);
    let stable = |message: &str| {
        message
            .replace(&format!("{}/", root.display()), "")
            .replace(&format!("{}/", dir.path().display()), "")
    };

    let mut counts = Counts::default();
    let mut failures = vec![];
    let mut refusals = vec![];
    let mut applied = vec![];
    let mut inexact = vec![];
    for (file, line, name, destinations) in declarations(&root, package, &original) {
        for destination in destinations {
            counts.tried += 1;
            let label = format!("`{name}` from {file} to {destination}");
            match plan(&root, &original, &file, line, &destination) {
                Err(message) => {
                    counts.refused += 1;
                    // A refusal is repeatable.
                    match plan(&root, &original, &file, line, &destination) {
                        Err(again) if again == message => {}
                        other => failures
                            .push(format!("{label}: refused with {message:?}, then {other:?}")),
                    }
                    refusals.push(format!("{label}: {}", stable(&message)));
                }
                Ok((files, api_changes)) => {
                    counts.applied += 1;
                    // The reverse move, planned against the moved text.
                    let back = line_of(&root.join(&destination), &files[&destination], &name)
                        .map(|line| plan(&root, &files, &destination, line, &file));
                    match back {
                        Some(Ok((back, _))) => {
                            counts.round_trips += 1;
                            match round_trip(&root, &original, &back, &file, &destination, &name) {
                                Ok(true) => counts.exact_round_trips += 1,
                                Ok(false) => {
                                    inexact
                                        .push(format!("{label}\n{}", difference(&original, &back)));
                                    // The text moved back keeps the output.
                                    applied.push(Applied {
                                        label: format!("{label} and back"),
                                        files: back,
                                        api_changes: vec![],
                                    });
                                }
                                Err(changed) => {
                                    failures.push(format!("{label}: moving it back {changed}"))
                                }
                            }
                        }
                        Some(Err(message)) => {
                            refusals.push(format!("{label}: back refused: {}", stable(&message)))
                        }
                        None => failures.push(format!("{label}: not found in its destination")),
                    }
                    applied.push(Applied {
                        label,
                        files,
                        api_changes,
                    });
                }
            }
        }
    }
    // Planning reads the files and never writes or creates one.
    assert_eq!(listing(&root), on_disk);

    if swap {
        counts.swapped_checks = applied.len();
    }
    // Build and run each applied move in its own copy, a few at a time.
    let queue = Mutex::new(applied.into_iter());
    let found = Mutex::new(vec![]);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                loop {
                    let Some(moved) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let copy = tempfile::tempdir().unwrap();
                    write(copy.path(), &moved.files);
                    let expected = expected_output(&output, &moved.api_changes);
                    let failure = match behaviour(copy.path(), tests) {
                        Ok(actual) if actual == expected => match warnings(copy.path()) {
                            Ok(after) if after == warned => match swap {
                                false => continue,
                                true => match checks(&swapped(&moved.files)) {
                                    Ok(()) => continue,
                                    Err(error) => format!(
                                        "{}: with the other arms selected: {error}",
                                        moved.label
                                    ),
                                },
                            },
                            Ok(after) => {
                                format!("{}: warned {after:?}, not {warned:?}", moved.label)
                            }
                            Err(error) => format!("{}: check failed: {error}", moved.label),
                        },
                        Ok(actual) => {
                            format!("{}: printed {actual:?}, expected {expected:?}", moved.label)
                        }
                        Err(error) => format!("{}: {error}\n{:#?}", moved.label, moved.files),
                    };
                    found.lock().unwrap().push(failure);
                }
            });
        }
    });
    failures.extend(found.into_inner().unwrap());
    let report = Report {
        counts,
        refusals: refusals.join("\n"),
        inexact: inexact.concat(),
        failures,
    };
    eprintln!(
        "{:?}\nrefusals:\n{}\ninexact:\n{}",
        report.counts, report.refusals, report.inexact
    );
    report
}

#[test]
fn every_move_in_a_package_with_shadowed_names_keeps_its_output() {
    let report = every_move(SHADOWING, "", false);
    check(
        report,
        Counts {
            tried: 33,
            applied: 18,
            refused: 15,
            round_trips: 18,
            exact_round_trips: 5,
            swapped_checks: 0,
        },
        r##"`SCALE` from src/geometry.plenty to src/main.plenty: after moving `SCALE`, the package would not check: src/main.plenty:1:1: src/geometry.plenty:2:1: circular import: src/main.plenty -> src/geometry.plenty -> src/main.plenty
`SCALE` from src/geometry.plenty to src/report.plenty: after moving `SCALE`, the package would not check: src/main.plenty:1:1: src/geometry.plenty:2:1: src/report.plenty:1:1: circular import: src/geometry.plenty -> src/report.plenty -> src/geometry.plenty
`Point` from src/geometry.plenty to src/main.plenty: moving `Point` would make `geometry` and `main` import each other: `Point` uses `SCALE` and the rest of `geometry` uses `Point`
`Point` from src/geometry.plenty to src/report.plenty: moving `Point` would make `geometry` and `report` import each other: `Point` uses `SCALE` and the rest of `geometry` uses `Point`
`Point` from src/geometry.plenty to src/units.plenty: moving `Point` would make `geometry` and `units` import each other: `Point` uses `SCALE` and the rest of `geometry` uses `Point`
`scale` from src/geometry.plenty to src/main.plenty: after moving `scale`, the package would not check: src/main.plenty:2:1: src/report.plenty:3:1: circular import: src/main.plenty -> src/report.plenty -> src/main.plenty
`offset` from src/geometry.plenty to src/main.plenty: after moving `offset`, the package would not check: src/main.plenty:1:1: src/geometry.plenty:2:1: circular import: src/main.plenty -> src/geometry.plenty -> src/main.plenty
`offset` from src/geometry.plenty to src/report.plenty: after moving `offset`, the package would not check: src/main.plenty:1:1: src/geometry.plenty:2:1: src/report.plenty:1:1: circular import: src/geometry.plenty -> src/report.plenty -> src/geometry.plenty
`main` from src/main.plenty to src/geometry.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`main` from src/main.plenty to src/report.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`main` from src/main.plenty to src/units.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`Unit` from src/units.plenty to src/main.plenty: after moving `Unit`, the package would not check: src/main.plenty:2:1: src/report.plenty:3:1: src/units.plenty:1:1: circular import: src/main.plenty -> src/report.plenty -> src/units.plenty -> src/main.plenty
`Unit` from src/units.plenty to src/report.plenty: after moving `Unit`, the package would not check: src/main.plenty:2:1: src/report.plenty:3:1: src/units.plenty:1:1: circular import: src/report.plenty -> src/units.plenty -> src/report.plenty
`label` from src/units.plenty to src/main.plenty: `label` uses `units.Unit`, but `main` already binds `Unit`
`label` from src/units.plenty to src/report.plenty: `label` uses `units.Unit`, but `report` already binds `Unit`"##,
        r##"`SCALE` from src/geometry.plenty to src/units.plenty
  src/geometry.plenty
    - pub const SCALE: i64 = 10
    + pub const SCALE: i64 = 10
`scale` from src/geometry.plenty to src/report.plenty
  src/geometry.plenty
    - pub def scale(p: &Point) -> Result[i64, OverflowError[i64]]:
    -     Ok(p.y * SCALE)
    + pub def scale(p: &Point) -> Result[i64, OverflowError[i64]]:
    +     Ok(p.y * SCALE)
`scale` from src/geometry.plenty to src/units.plenty
  src/geometry.plenty
    - pub def scale(p: &Point) -> Result[i64, OverflowError[i64]]:
    -     Ok(p.y * SCALE)
    + pub def scale(p: &Point) -> Result[i64, OverflowError[i64]]:
    +     Ok(p.y * SCALE)
  src/report.plenty
    - import geometry as g
    + import geometry
    -     scaled = g.scale(p)?
    +     scaled = geometry.scale(p)?
`offset` from src/geometry.plenty to src/units.plenty
  src/geometry.plenty
    - # The parameter shadows the module's constant.
    - def offset(SCALE: i64) -> i64:
    -     SCALE
    + # The parameter shadows the module's constant.
    + pub def offset(SCALE: i64) -> i64:
    +     SCALE
`shifted` from src/geometry.plenty to src/main.plenty
  src/geometry.plenty
    - def offset(SCALE: i64) -> i64:
    + pub def offset(SCALE: i64) -> i64:
    - pub def shifted(x: i64) -> Result[i64, OverflowError[i64]]:
    -     scale = 3
    -     Ok(offset(x) + scale)
    + pub def shifted(x: i64) -> Result[i64, OverflowError[i64]]:
    +     scale = 3
    +     Ok(offset(x) + scale)
`shifted` from src/geometry.plenty to src/report.plenty
  src/geometry.plenty
    - def offset(SCALE: i64) -> i64:
    + pub def offset(SCALE: i64) -> i64:
    - pub def shifted(x: i64) -> Result[i64, OverflowError[i64]]:
    -     scale = 3
    -     Ok(offset(x) + scale)
    + pub def shifted(x: i64) -> Result[i64, OverflowError[i64]]:
    +     scale = 3
    +     Ok(offset(x) + scale)
`shifted` from src/geometry.plenty to src/units.plenty
  src/geometry.plenty
    - def offset(SCALE: i64) -> i64:
    + pub def offset(SCALE: i64) -> i64:
    - pub def shifted(x: i64) -> Result[i64, OverflowError[i64]]:
    -     scale = 3
    -     Ok(offset(x) + scale)
    + pub def shifted(x: i64) -> Result[i64, OverflowError[i64]]:
    +     scale = 3
    +     Ok(offset(x) + scale)
`BANNER` from src/report.plenty to src/geometry.plenty
  src/report.plenty
    + pub def describe(p: &P, u: &Unit) -> Result[str, AllocError | OverflowError[i64]]:
    +     scaled = g.scale(p)?
    +     f"{p.x},{p.y} by {SCALE} is {scaled} {label(u)}"
    - pub def describe(p: &P, u: &Unit) -> Result[str, AllocError | OverflowError[i64]]:
    -     scaled = g.scale(p)?
    -     f"{p.x},{p.y} by {SCALE} is {scaled} {label(u)}"
`BANNER` from src/report.plenty to src/main.plenty
  src/report.plenty
    + pub def describe(p: &P, u: &Unit) -> Result[str, AllocError | OverflowError[i64]]:
    +     scaled = g.scale(p)?
    +     f"{p.x},{p.y} by {SCALE} is {scaled} {label(u)}"
    - pub def describe(p: &P, u: &Unit) -> Result[str, AllocError | OverflowError[i64]]:
    -     scaled = g.scale(p)?
    -     f"{p.x},{p.y} by {SCALE} is {scaled} {label(u)}"
`BANNER` from src/report.plenty to src/units.plenty
  src/report.plenty
    + pub def describe(p: &P, u: &Unit) -> Result[str, AllocError | OverflowError[i64]]:
    +     scaled = g.scale(p)?
    +     f"{p.x},{p.y} by {SCALE} is {scaled} {label(u)}"
    - pub def describe(p: &P, u: &Unit) -> Result[str, AllocError | OverflowError[i64]]:
    -     scaled = g.scale(p)?
    -     f"{p.x},{p.y} by {SCALE} is {scaled} {label(u)}"
`describe` from src/report.plenty to src/geometry.plenty
  src/report.plenty
    - import geometry as g
    - from geometry import Point as P, SCALE
    + from geometry import Point, SCALE, scale
    - pub def describe(p: &P, u: &Unit) -> Result[str, AllocError | OverflowError[i64]]:
    -     scaled = g.scale(p)?
    + pub def describe(p: &Point, u: &Unit) -> Result[str, AllocError | OverflowError[i64]]:
    +     scaled = scale(p)?
`describe` from src/report.plenty to src/main.plenty
  src/report.plenty
    + from units import Unit, label
    - from units import Unit, label
`Unit` from src/units.plenty to src/geometry.plenty
  src/units.plenty
    - pub enum Unit:
    -     Metre
    -     Foot
    + pub enum Unit:
    +     Metre
    +     Foot
"##,
    );
}

#[test]
fn every_move_in_a_package_with_nested_crlf_modules_keeps_its_output() {
    let report = every_move(NESTED, "", true);
    check(
        report,
        Counts {
            tried: 21,
            applied: 9,
            refused: 12,
            round_trips: 5,
            exact_round_trips: 1,
            swapped_checks: 0,
        },
        r##"`version` from src/lib.plenty to src/main.plenty: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`version` from src/lib.plenty to src/text/format.plenty: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`version` from src/lib.plenty to src/text/strings.plenty: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`main` from src/main.plenty to src/lib.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`main` from src/main.plenty to src/text/format.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`main` from src/main.plenty to src/text/strings.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`POEM` from src/text/format.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`Secret` from src/text/format.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`Secret` from src/text/format.plenty to src/main.plenty: after moving `Secret`, the package would not check: src/main.plenty:3:1: src/text/format.plenty:2:1: circular import: src/main.plenty -> src/text/format.plenty -> src/main.plenty
`wrap` from src/text/format.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`wrap` from src/text/format.plenty to src/main.plenty: `wrap` uses `text.format.Secret`, but `main` already binds `Secret`
`GREETING` from src/text/strings.plenty to src/lib.plenty: `GREETING` includes a file by a path relative to src/text/; move it to a module in that directory, or change the path first
`GREETING` from src/text/strings.plenty to src/main.plenty: `GREETING` includes a file by a path relative to src/text/; move it to a module in that directory, or change the path first
`GREETING` from src/text/strings.plenty to src/text/format.plenty: after moving `GREETING`, the package would not check: src/main.plenty:2:1: src/text/strings.plenty:1:1: src/text/format.plenty:1:1: circular import: src/text/strings.plenty -> src/text/format.plenty -> src/text/strings.plenty
`greet` from src/text/strings.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`greet` from src/text/strings.plenty to src/main.plenty: after moving `greet`, the package would not check: src/main.plenty:3:1: src/text/format.plenty:1:1: circular import: src/main.plenty -> src/text/format.plenty -> src/main.plenty"##,
        r##"`POEM` from src/text/format.plenty to src/main.plenty
  src/text/format.plenty
    - pub const POEM: str = """one
    - two
    - """
    + pub const POEM: str = """one
    + two
    + """
`POEM` from src/text/format.plenty to src/text/strings.plenty
  src/text/format.plenty
    - pub const POEM: str = """one
    - two
    - """
    + pub const POEM: str = """one
    + two
    + """
`Secret` from src/text/format.plenty to src/text/strings.plenty
  src/text/format.plenty
    + pub def wrap(s: &Secret) -> Result[str, AllocError]:
    +     f"[{s.code}] {greet()}"
    - pub def wrap(s: &Secret) -> Result[str, AllocError]:
    -     f"[{s.code}] {greet()}"
`greet` from src/text/strings.plenty to src/text/format.plenty
  src/main.plenty
    - import text.strings as strings
    + import text.strings
    -     print(strings.greet(), POEM.byte_len())?
    +     print(text.strings.greet(), POEM.byte_len())?
"##,
    );
}

#[test]
fn every_move_in_a_package_with_aliases_protocols_and_conditionals_keeps_its_output() {
    let report = every_move(VARIETY, "", true);
    check(
        report,
        Counts {
            tried: 58,
            applied: 38,
            refused: 20,
            round_trips: 24,
            exact_round_trips: 3,
            swapped_checks: 59,
        },
        r##"`firsts` from src/calc.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`clamp` from src/calc.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`cap` from src/calc.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`platform` from src/calc.plenty to src/lib.plenty: src/calc.plenty:23: `platform` has an `if const` block, whose unselected arms the move cannot check
`platform` from src/calc.plenty to src/limits.plenty: src/calc.plenty:23: `platform` has an `if const` block, whose unselected arms the move cannot check
`platform` from src/calc.plenty to src/main.plenty: src/calc.plenty:23: `platform` has an `if const` block, whose unselected arms the move cannot check
`platform` from src/calc.plenty to src/shapes.plenty: src/calc.plenty:23: `platform` has an `if const` block, whose unselected arms the move cannot check
`version` from src/lib.plenty to src/calc.plenty: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`version` from src/lib.plenty to src/limits.plenty: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`version` from src/lib.plenty to src/main.plenty: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`version` from src/lib.plenty to src/shapes.plenty: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`LIMIT` from src/limits.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`LIMIT` from src/limits.plenty to src/main.plenty: after moving `LIMIT`, the package would not check: src/main.plenty:1:1: src/calc.plenty:1:1: circular import: src/main.plenty -> src/calc.plenty -> src/main.plenty
`main` from src/main.plenty to src/calc.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`main` from src/main.plenty to src/lib.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`main` from src/main.plenty to src/limits.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`main` from src/main.plenty to src/shapes.plenty: src/main.plenty runs `main`; a target's `main` cannot move
`has_pid` from src/native.plentyi to src/calc.plenty: `has_pid` uses a declaration of `native` the move cannot import
`has_pid` from src/native.plentyi to src/lib.plenty: `has_pid` uses a declaration of `native` the move cannot import
`has_pid` from src/native.plentyi to src/limits.plenty: `has_pid` uses a declaration of `native` the move cannot import
`has_pid` from src/native.plentyi to src/main.plenty: `has_pid` uses a declaration of `native` the move cannot import
`has_pid` from src/native.plentyi to src/shapes.plenty: `has_pid` uses a declaration of `native` the move cannot import
`pid_label` from src/native.plentyi to src/calc.plenty: back refused: the destination must be a module of the same package's src/
`pid_label` from src/native.plentyi to src/lib.plenty: back refused: the destination must be a module of the same package's src/
`pid_label` from src/native.plentyi to src/limits.plenty: back refused: the destination must be a module of the same package's src/
`pid_label` from src/native.plentyi to src/main.plenty: back refused: the destination must be a module of the same package's src/
`pid_label` from src/native.plentyi to src/shapes.plenty: back refused: the destination must be a module of the same package's src/
`Side` from src/shapes.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`Side` from src/shapes.plenty to src/main.plenty: after moving `Side`, the package would not check: src/main.plenty:4:1: src/shapes.plenty:1:1: circular import: src/main.plenty -> src/shapes.plenty -> src/main.plenty
`Square` from src/shapes.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`Strip` from src/shapes.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`Measured` from src/shapes.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`Measured` from src/shapes.plenty to src/main.plenty: after moving `Measured`, the package would not check: src/main.plenty:4:1: src/shapes.plenty:1:1: circular import: src/main.plenty -> src/shapes.plenty -> src/main.plenty
`doubled` from src/shapes.plenty to src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported"##,
        r##"`firsts` from src/calc.plenty to src/limits.plenty
  src/calc.plenty
    - # The comprehension binds its own `LIMIT`.
    - pub def firsts(n: i64) -> Result[list[i64], AllocError]:
    -     [LIMIT for LIMIT in range(n)]
    + # The comprehension binds its own `LIMIT`.
    + pub def firsts(n: i64) -> Result[list[i64], AllocError]:
    +     [LIMIT for LIMIT in range(n)]
`firsts` from src/calc.plenty to src/main.plenty
  src/calc.plenty
    - # The comprehension binds its own `LIMIT`.
    - pub def firsts(n: i64) -> Result[list[i64], AllocError]:
    -     [LIMIT for LIMIT in range(n)]
    + # The comprehension binds its own `LIMIT`.
    + pub def firsts(n: i64) -> Result[list[i64], AllocError]:
    +     [LIMIT for LIMIT in range(n)]
`firsts` from src/calc.plenty to src/shapes.plenty
  src/calc.plenty
    - # The comprehension binds its own `LIMIT`.
    - pub def firsts(n: i64) -> Result[list[i64], AllocError]:
    -     [LIMIT for LIMIT in range(n)]
    + # The comprehension binds its own `LIMIT`.
    + pub def firsts(n: i64) -> Result[list[i64], AllocError]:
    +     [LIMIT for LIMIT in range(n)]
`clamp` from src/calc.plenty to src/limits.plenty
  src/calc.plenty
    - # So does the case pattern.
    - pub def clamp(value: Option[i64]) -> i64:
    -     match value:
    -         case Some(LIMIT):
    -             LIMIT
    -         case Nothing:
    -             0
    + # So does the case pattern.
    + pub def clamp(value: Option[i64]) -> i64:
    +     match value:
    +         case Some(LIMIT):
    +             LIMIT
    +         case Nothing:
    +             0
`clamp` from src/calc.plenty to src/main.plenty
  src/calc.plenty
    - # So does the case pattern.
    - pub def clamp(value: Option[i64]) -> i64:
    -     match value:
    -         case Some(LIMIT):
    -             LIMIT
    -         case Nothing:
    -             0
    + # So does the case pattern.
    + pub def clamp(value: Option[i64]) -> i64:
    +     match value:
    +         case Some(LIMIT):
    +             LIMIT
    +         case Nothing:
    +             0
`clamp` from src/calc.plenty to src/shapes.plenty
  src/calc.plenty
    - # So does the case pattern.
    - pub def clamp(value: Option[i64]) -> i64:
    -     match value:
    -         case Some(LIMIT):
    -             LIMIT
    -         case Nothing:
    -             0
    + # So does the case pattern.
    + pub def clamp(value: Option[i64]) -> i64:
    +     match value:
    +         case Some(LIMIT):
    +             LIMIT
    +         case Nothing:
    +             0
`cap` from src/calc.plenty to src/limits.plenty
  src/calc.plenty
    - pub def cap(x: i64) -> i64:
    -     LIMIT if x > LIMIT else x
    + pub def cap(x: i64) -> i64:
    +     LIMIT if x > LIMIT else x
`cap` from src/calc.plenty to src/main.plenty
  src/calc.plenty
    - pub def cap(x: i64) -> i64:
    -     LIMIT if x > LIMIT else x
    + pub def cap(x: i64) -> i64:
    +     LIMIT if x > LIMIT else x
`cap` from src/calc.plenty to src/shapes.plenty
  src/calc.plenty
    - pub def cap(x: i64) -> i64:
    -     LIMIT if x > LIMIT else x
    + pub def cap(x: i64) -> i64:
    +     LIMIT if x > LIMIT else x
`LIMIT` from src/limits.plenty to src/calc.plenty
  src/main.plenty
    - import limits
    + import limits
`LIMIT` from src/limits.plenty to src/shapes.plenty
  src/main.plenty
    - import limits
    + import limits
`Side` from src/shapes.plenty to src/calc.plenty
  src/shapes.plenty
    - pub type Side = i64
    + pub type Side = i64
`Side` from src/shapes.plenty to src/limits.plenty
  src/shapes.plenty
    - pub type Side = i64
    + pub type Side = i64
`Square` from src/shapes.plenty to src/calc.plenty
  src/shapes.plenty
    - # Two classes with a method of the same name.
    - pub class Square:
    -     pub side: Side
    -     pub def area(self) -> Result[i64, OverflowError[i64]]:
    -         Ok(self.side * self.side)
    + # Two classes with a method of the same name.
    + pub class Square:
    +     pub side: Side
    +     pub def area(self) -> Result[i64, OverflowError[i64]]:
    +         Ok(self.side * self.side)
`Square` from src/shapes.plenty to src/limits.plenty
  src/shapes.plenty
    - # Two classes with a method of the same name.
    - pub class Square:
    -     pub side: Side
    -     pub def area(self) -> Result[i64, OverflowError[i64]]:
    -         Ok(self.side * self.side)
    + # Two classes with a method of the same name.
    + pub class Square:
    +     pub side: Side
    +     pub def area(self) -> Result[i64, OverflowError[i64]]:
    +         Ok(self.side * self.side)
`Square` from src/shapes.plenty to src/main.plenty
  src/shapes.plenty
    - # Two classes with a method of the same name.
    - pub class Square:
    -     pub side: Side
    -     pub def area(self) -> Result[i64, OverflowError[i64]]:
    -         Ok(self.side * self.side)
    + # Two classes with a method of the same name.
    + pub class Square:
    +     pub side: Side
    +     pub def area(self) -> Result[i64, OverflowError[i64]]:
    +         Ok(self.side * self.side)
`Strip` from src/shapes.plenty to src/calc.plenty
  src/shapes.plenty
    - pub class Strip:
    -     pub width: Side
    -     pub length: Side
    -     pub def area(self) -> Result[i64, OverflowError[i64]]:
    -         Ok(self.width * self.length)
    + pub class Strip:
    +     pub width: Side
    +     pub length: Side
    +     pub def area(self) -> Result[i64, OverflowError[i64]]:
    +         Ok(self.width * self.length)
`Strip` from src/shapes.plenty to src/limits.plenty
  src/shapes.plenty
    - pub class Strip:
    -     pub width: Side
    -     pub length: Side
    -     pub def area(self) -> Result[i64, OverflowError[i64]]:
    -         Ok(self.width * self.length)
    + pub class Strip:
    +     pub width: Side
    +     pub length: Side
    +     pub def area(self) -> Result[i64, OverflowError[i64]]:
    +         Ok(self.width * self.length)
`Strip` from src/shapes.plenty to src/main.plenty
  src/shapes.plenty
    - pub class Strip:
    -     pub width: Side
    -     pub length: Side
    -     pub def area(self) -> Result[i64, OverflowError[i64]]:
    -         Ok(self.width * self.length)
    + pub class Strip:
    +     pub width: Side
    +     pub length: Side
    +     pub def area(self) -> Result[i64, OverflowError[i64]]:
    +         Ok(self.width * self.length)
`Measured` from src/shapes.plenty to src/calc.plenty
  src/shapes.plenty
    + pub def doubled[T: Measured](item: &T) -> Result[i64, OverflowError[i64]]:
    +     Ok(item.area()? * 2)
    - pub def doubled[T: Measured](item: &T) -> Result[i64, OverflowError[i64]]:
    -     Ok(item.area()? * 2)
`Measured` from src/shapes.plenty to src/limits.plenty
  src/shapes.plenty
    + pub def doubled[T: Measured](item: &T) -> Result[i64, OverflowError[i64]]:
    +     Ok(item.area()? * 2)
    - pub def doubled[T: Measured](item: &T) -> Result[i64, OverflowError[i64]]:
    -     Ok(item.area()? * 2)
"##,
    );
}

#[test]
fn every_move_in_a_workspace_library_keeps_its_dependents_output() {
    let report = every_move(WORKSPACE, "tools/", true);
    check(
        report,
        Counts {
            tried: 10,
            applied: 4,
            refused: 6,
            round_trips: 3,
            exact_round_trips: 1,
            swapped_checks: 0,
        },
        r##"`Color` from tools/src/colors.plenty to tools/src/lib.plenty: after moving `Color`, the package would not check: tools/src/lib.plenty:1:1: tools/src/shapes.plenty:1:1: tools/src/colors.plenty:1:1: circular import: tools/src/lib.plenty -> tools/src/shapes.plenty -> tools/src/colors.plenty -> tools/src/lib.plenty
`Color` from tools/src/colors.plenty to tools/src/shapes.plenty: after moving `Color`, the package would not check: tools/src/lib.plenty:1:1: tools/src/shapes.plenty:1:1: tools/src/colors.plenty:1:1: circular import: tools/src/shapes.plenty -> tools/src/colors.plenty -> tools/src/shapes.plenty
`name` from tools/src/colors.plenty to tools/src/lib.plenty: after moving `name`, the package would not check: tools/src/lib.plenty:1:1: tools/src/shapes.plenty:2:1: circular import: tools/src/lib.plenty -> tools/src/shapes.plenty -> tools/src/lib.plenty
`origin` from tools/src/lib.plenty to tools/src/colors.plenty: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`origin` from tools/src/lib.plenty to tools/src/shapes.plenty: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`Point` from tools/src/shapes.plenty to tools/src/lib.plenty: back refused: `lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported
`describe` from tools/src/shapes.plenty to tools/src/lib.plenty: `describe` uses `shapes.Point`, but `lib` already binds `Point`"##,
        r##"`Point` from tools/src/shapes.plenty to tools/src/colors.plenty
  tools/src/shapes.plenty
    + pub def describe(p: &Point) -> Result[str, AllocError]:
    +     f"{p.x} in {colors.name(&colors.Color.Red)}"
    - pub def describe(p: &Point) -> Result[str, AllocError]:
    -     f"{p.x} in {colors.name(&colors.Color.Red)}"
`describe` from tools/src/shapes.plenty to tools/src/colors.plenty
  tools/src/shapes.plenty
    - import colors
    + from colors import Color, name
    -     f"{p.x} in {colors.name(&colors.Color.Red)}"
    +     f"{p.x} in {name(&Color.Red)}"
"##,
    );
}

fn check(report: Report, counts: Counts, refusals: &str, inexact: &str) {
    assert!(
        report.failures.is_empty(),
        "{}",
        report.failures.join("\n\n")
    );
    assert_eq!(report.counts, counts);
    assert_eq!(report.refusals, refusals);
    assert_eq!(report.inexact, inexact);
}
