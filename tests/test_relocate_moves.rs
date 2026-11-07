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

/// A library package with tests and a workspace member that depends on it.
const WORKSPACE: &[(&str, &str)] = &[
    (
        "plenty.toml",
        "manifest-version = 1\n\n[workspace]\nmembers = [\"tools\", \"app\"]\n",
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
        "manifest-version = 1\n\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\ntools = { path = \"../tools\" }\n",
    ),
    (
        "app/src/main.plenty",
        "\
import tools
import tools.colors as c
from tools.shapes import Point, describe

def main() -> Result[(), AllocError | IoError]:
    p = Point(x=5)
    print(p, describe(&p)?, tools.origin())?
    print(c.Color.Blue, c.name(&c.Color.Blue))?
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
        if !path.starts_with(&src) || !path.ends_with(".plenty") {
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

/// How a round trip that is not exact differs: by where the declaration is
/// in its module and whether it is `pub`, or also by other text.
fn difference(original: &Files, back: &Files, file: &str) -> &'static str {
    let lines = |text: &str| {
        let mut lines: Vec<String> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.strip_prefix("pub ").unwrap_or(line).to_owned())
            .collect();
        lines.sort();
        lines
    };
    let other_files_same = original
        .iter()
        .all(|(path, text)| path == file || back[path] == *text);
    if other_files_same && lines(&original[file]) == lines(&back[file]) {
        "position"
    } else {
        "imports"
    }
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
}

struct Applied {
    label: String,
    files: Files,
    api_changes: Vec<String>,
}

fn every_move(
    fixture: &[(&str, &str)],
    package: &str,
    tests: bool,
) -> (Counts, Vec<String>, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let original: Files = fixture
        .iter()
        .map(|(path, text)| (path.to_string(), text.to_string()))
        .collect();
    write(&root, &original);
    let output = behaviour(&root, tests).unwrap();
    let on_disk = |root: &Path| -> Files {
        original
            .keys()
            .map(|path| {
                (
                    path.clone(),
                    std::fs::read_to_string(root.join(path)).unwrap(),
                )
            })
            .collect()
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
                    refusals.push(format!("{label}: {message}"));
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
                                    inexact.push(format!(
                                        "{label}: {}",
                                        difference(&original, &back, &file)
                                    ));
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
                        Some(Err(_)) => {}
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
    // Planning reads the files and never writes them.
    assert_eq!(on_disk(&root), original);

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
                        Ok(actual) if actual == expected => continue,
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
    eprintln!(
        "{counts:?}\n{}\ninexact:\n{}",
        refusals.join("\n"),
        inexact.join("\n")
    );
    (counts, inexact, failures)
}

#[test]
fn every_move_in_a_package_with_shadowed_names_keeps_its_output() {
    let (counts, inexact, failures) = every_move(SHADOWING, "", false);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert_eq!(
        counts,
        Counts {
            tried: 33,
            applied: 17,
            refused: 16,
            round_trips: 17,
            exact_round_trips: 5,
        }
    );
    assert_eq!(
        inexact,
        [
            "`SCALE` from src/geometry.plenty to src/units.plenty: position",
            "`scale` from src/geometry.plenty to src/units.plenty: imports",
            "`offset` from src/geometry.plenty to src/units.plenty: position",
            "`shifted` from src/geometry.plenty to src/main.plenty: position",
            "`shifted` from src/geometry.plenty to src/report.plenty: position",
            "`shifted` from src/geometry.plenty to src/units.plenty: position",
            "`BANNER` from src/report.plenty to src/geometry.plenty: position",
            "`BANNER` from src/report.plenty to src/main.plenty: position",
            "`BANNER` from src/report.plenty to src/units.plenty: position",
            "`describe` from src/report.plenty to src/geometry.plenty: imports",
            "`describe` from src/report.plenty to src/main.plenty: position",
            "`Unit` from src/units.plenty to src/geometry.plenty: position",
        ]
    );
}

#[test]
fn every_move_in_a_package_with_nested_crlf_modules_keeps_its_output() {
    let (counts, inexact, failures) = every_move(NESTED, "", true);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert_eq!(
        counts,
        Counts {
            tried: 21,
            applied: 9,
            refused: 12,
            round_trips: 5,
            exact_round_trips: 1,
        }
    );
    assert_eq!(
        inexact,
        [
            "`POEM` from src/text/format.plenty to src/main.plenty: position",
            "`POEM` from src/text/format.plenty to src/text/strings.plenty: position",
            "`Secret` from src/text/format.plenty to src/text/strings.plenty: position",
            "`greet` from src/text/strings.plenty to src/text/format.plenty: imports",
        ]
    );
}

#[test]
fn every_move_in_a_workspace_library_keeps_its_dependents_output() {
    let (counts, inexact, failures) = every_move(WORKSPACE, "tools/", true);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert_eq!(
        counts,
        Counts {
            tried: 10,
            applied: 4,
            refused: 6,
            round_trips: 3,
            exact_round_trips: 1,
        }
    );
    assert_eq!(
        inexact,
        [
            "`Point` from tools/src/shapes.plenty to tools/src/colors.plenty: position",
            "`describe` from tools/src/shapes.plenty to tools/src/colors.plenty: imports",
        ]
    );
}
