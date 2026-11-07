//! Moving a top-level declaration to another module (issue #822), planned by
//! `plenty::ide::relocate` against packages written to a temporary directory.
use plenty::ide::relocate;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const MANIFEST: &str = "manifest-version = 1\n\n[package]\nname = \"tools\"\nversion = \"0.1.0\"\n";

fn package(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("plenty.toml"), MANIFEST).unwrap();
    for (path, text) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

/// Every file below `dir`, by relative path.
fn snapshot(dir: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path.strip_prefix(root).unwrap().display().to_string();
                out.insert(relative, std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

struct Moved {
    /// The changed files' new text, by path relative to the package.
    files: BTreeMap<String, String>,
    api_changes: Vec<String>,
}

/// Plan moving the declaration on `line` of `file` to `destination`, and
/// apply the plan's edits in memory. Planning never writes a file.
fn plan_with(
    dir: &Path,
    file: &str,
    line: u32,
    destination: &str,
    overlay: &[(&str, &str)],
) -> Result<Moved, String> {
    let root = dir.canonicalize().unwrap();
    let overlay: HashMap<PathBuf, String> = overlay
        .iter()
        .map(|(path, text)| (root.join(path), text.to_string()))
        .collect();
    let before = snapshot(dir);
    let plan = relocate::plan(&root.join(file), line, &root.join(destination), &overlay);
    assert_eq!(before, snapshot(dir), "planning a move wrote files");
    let plan = plan?;
    let mut files = BTreeMap::new();
    for edits in &plan.files {
        let text = match overlay.get(&edits.path) {
            Some(text) => text.clone(),
            None => std::fs::read_to_string(&edits.path).unwrap(),
        };
        let relative = edits
            .path
            .strip_prefix(&root)
            .unwrap()
            .display()
            .to_string();
        files.insert(relative, relocate::apply(&text, &edits.edits).unwrap());
    }
    Ok(Moved {
        files,
        api_changes: plan.api_changes,
    })
}

fn plan(dir: &Path, file: &str, line: u32, destination: &str) -> Result<Moved, String> {
    plan_with(dir, file, line, destination, &[])
}

fn refused(dir: &Path, file: &str, line: u32, destination: &str) -> String {
    match plan(dir, file, line, destination) {
        Ok(moved) => panic!("the move was not refused: {:#?}", moved.files),
        Err(message) => message,
    }
}

const SHAPES: &str = "\
import std.math as m

# A point on the grid.
pub class Point:
    pub x: i64
    pub y: i64

pub def norm(p: &Point) -> f64:
    \"\"\"The distance from the origin's column.\"\"\"
    m.fabs(f64(p.x))
";

const MAIN: &str = "\
import shapes
from shapes import Point as P

def show(p: shapes.Point) -> ():
    pass

def main() -> ():
    a = P(x=1, y=2)
    b = shapes.Point(x=3, y=4)
    show(a)
    show(b)
";

const TEST: &str = "\
import tools.shapes

def test_point() -> ():
    p = tools.shapes.Point(x=1, y=2)
    pass
";

const GEOMETRY: &str = "pub def unit() -> i64:\n    1\n";

#[test]
fn a_class_moves_with_its_comment_and_every_caller_follows() {
    let dir = package(&[
        ("src/shapes.plenty", SHAPES),
        (
            "src/main.plenty",
            &MAIN.replace("    show(a)\n", "    n = shapes.norm(&a)\n    show(a)\n"),
        ),
        ("src/geometry.plenty", GEOMETRY),
        ("src/lib.plenty", "pub def version() -> i64:\n    1\n"),
        ("tests/points.plenty", TEST),
    ]);
    let moved = plan(dir.path(), "src/shapes.plenty", 5, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.api_changes,
        [
            "moves public `tools.shapes.Point` to `tools.geometry.Point`; packages outside this workspace that import it must change"
        ]
    );
    let expected = BTreeMap::from([
        (
            "src/geometry.plenty".to_string(),
            "pub def unit() -> i64:\n    1\n\n# A point on the grid.\npub class Point:\n    pub x: i64\n    pub y: i64\n".to_string(),
        ),
        (
            "src/shapes.plenty".to_string(),
            "import std.math as m\nfrom geometry import Point\n\npub def norm(p: &Point) -> f64:\n    \"\"\"The distance from the origin's column.\"\"\"\n    m.fabs(f64(p.x))\n".to_string(),
        ),
        (
            "src/main.plenty".to_string(),
            "import shapes\nfrom geometry import Point as P\nimport geometry\n\ndef show(p: geometry.Point) -> ():\n    pass\n\ndef main() -> ():\n    a = P(x=1, y=2)\n    b = geometry.Point(x=3, y=4)\n    n = shapes.norm(&a)\n    show(a)\n    show(b)\n".to_string(),
        ),
        (
            "tests/points.plenty".to_string(),
            "import tools.geometry\n\ndef test_point() -> ():\n    p = tools.geometry.Point(x=1, y=2)\n    pass\n".to_string(),
        ),
    ]);
    assert_eq!(moved.files, expected);
}

#[test]
fn a_function_takes_its_imports_and_imports_what_it_leaves_behind() {
    let dir = package(&[
        ("src/shapes.plenty", SHAPES),
        ("src/main.plenty", MAIN),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let moved = plan(dir.path(), "src/shapes.plenty", 9, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.files["src/geometry.plenty"],
        "import std.math as m\nfrom shapes import Point\n\npub def unit() -> i64:\n    1\n\npub def norm(p: &Point) -> f64:\n    \"\"\"The distance from the origin's column.\"\"\"\n    m.fabs(f64(p.x))\n"
    );
    assert_eq!(
        moved.files["src/shapes.plenty"],
        "# A point on the grid.\npub class Point:\n    pub x: i64\n    pub y: i64\n"
    );
    assert!(!moved.files.contains_key("src/main.plenty"));
}

#[test]
fn a_private_dependency_becomes_public_and_is_reported() {
    let dir = package(&[
        (
            "src/shapes.plenty",
            "def helper() -> ():\n    pass\n\npub def pick[T](x: T) -> T:\n    helper()\n    x\n",
        ),
        (
            "src/main.plenty",
            "from shapes import pick\n\ndef main() -> ():\n    x = pick(1)\n",
        ),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let moved = plan(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.api_changes,
        ["makes `shapes.helper` public, because the moved code uses it"]
    );
    assert_eq!(
        moved.files["src/shapes.plenty"],
        "pub def helper() -> ():\n    pass\n"
    );
    assert_eq!(
        moved.files["src/geometry.plenty"],
        "from shapes import helper\n\npub def unit() -> i64:\n    1\n\npub def pick[T](x: T) -> T:\n    helper()\n    x\n"
    );
    assert_eq!(
        moved.files["src/main.plenty"],
        "from geometry import pick\n\ndef main() -> ():\n    x = pick(1)\n"
    );
}

#[test]
fn a_private_declaration_used_elsewhere_becomes_public() {
    let dir = package(&[
        (
            "src/shapes.plenty",
            "def double(x: i64) -> i64:\n    x\n\npub def twice(x: i64) -> i64:\n    double(x)\n",
        ),
        (
            "src/main.plenty",
            "import shapes\n\ndef main() -> ():\n    x = shapes.twice(1)\n",
        ),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let moved = plan(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.api_changes,
        ["makes `double` public, because other modules use it"]
    );
    assert_eq!(
        moved.files["src/shapes.plenty"],
        "from geometry import double\n\npub def twice(x: i64) -> i64:\n    double(x)\n"
    );
    assert_eq!(
        moved.files["src/geometry.plenty"],
        "pub def unit() -> i64:\n    1\n\npub def double(x: i64) -> i64:\n    x\n"
    );
}

#[test]
fn moved_code_names_the_destinations_declarations_directly() {
    let dir = package(&[
        (
            "src/shapes.plenty",
            "from geometry import unit as u\nimport geometry\n\npub def two() -> i64:\n    u() + geometry.unit()\n",
        ),
        (
            "src/main.plenty",
            "import shapes\n\ndef main() -> ():\n    x = shapes.two()\n",
        ),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let moved = plan(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty");
    // `+` can overflow, which `two` does not declare.
    let message = moved.err().unwrap();
    assert!(message.starts_with("fix this error"), "{message}");

    std::fs::write(
        dir.path().join("src/shapes.plenty"),
        "from geometry import unit as u\nimport geometry\n\npub def two() -> (i64, i64):\n    (u(), geometry.unit())\n",
    )
    .unwrap();
    let moved = plan(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty").unwrap();
    assert_eq!(moved.files["src/shapes.plenty"], "");
    assert_eq!(
        moved.files["src/geometry.plenty"],
        "pub def unit() -> i64:\n    1\n\npub def two() -> (i64, i64):\n    (unit(), unit())\n"
    );
    assert_eq!(
        moved.files["src/main.plenty"],
        "import geometry\n\ndef main() -> ():\n    x = geometry.two()\n"
    );
}

#[test]
fn unsaved_text_is_what_moves() {
    let dir = package(&[
        ("src/shapes.plenty", "pub def old() -> i64:\n    1\n"),
        ("src/main.plenty", "def main() -> ():\n    pass\n"),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let shapes = "pub def edited() -> i64:\n    2\n";
    let main = "import shapes\n\ndef main() -> ():\n    x = shapes.edited()\n";
    let moved = plan_with(
        dir.path(),
        "src/shapes.plenty",
        1,
        "src/geometry.plenty",
        &[("src/shapes.plenty", shapes), ("src/main.plenty", main)],
    )
    .unwrap();
    assert_eq!(moved.files["src/shapes.plenty"], "");
    assert_eq!(
        moved.files["src/main.plenty"],
        "import geometry\n\ndef main() -> ():\n    x = geometry.edited()\n"
    );
}

#[test]
fn moving_a_public_library_declaration_reports_the_path_change() {
    let dir = package(&[
        (
            "src/lib.plenty",
            "import shapes\n\npub def origin() -> shapes.Point:\n    shapes.Point(x=0, y=0)\n",
        ),
        (
            "src/shapes.plenty",
            "pub class Point:\n    pub x: i64\n    pub y: i64\n",
        ),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let moved = plan(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.api_changes,
        [
            "moves public `tools.shapes.Point` to `tools.geometry.Point`; packages outside this workspace that import it must change"
        ]
    );
    assert!(moved.files["src/lib.plenty"].contains("geometry.Point"));
}

#[test]
fn a_cycle_is_refused() {
    // `twice` uses `shapes.double`, and `shapes` keeps using `twice`.
    let dir = package(&[
        (
            "src/shapes.plenty",
            "pub def double(x: i64) -> i64:\n    x\n\npub def twice(x: i64) -> i64:\n    double(x)\n\npub def thrice(x: i64) -> i64:\n    twice(x)\n",
        ),
        (
            "src/main.plenty",
            "import shapes\n\ndef main() -> ():\n    x = shapes.thrice(1)\n",
        ),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let message = refused(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty");
    assert!(message.contains("import each other"), "{message}");

    // The destination already imports the source, which would then import it.
    let dir = package(&[
        (
            "src/shapes.plenty",
            "pub def one() -> i64:\n    1\n\npub def uses() -> i64:\n    one()\n",
        ),
        (
            "src/main.plenty",
            "import geometry\n\ndef main() -> ():\n    x = geometry.via()\n",
        ),
        (
            "src/geometry.plenty",
            "import shapes\n\npub def via() -> i64:\n    shapes.uses()\n",
        ),
    ]);
    let message = refused(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty");
    assert!(message.contains("would not check"), "{message}");
    assert!(message.contains("circular import"), "{message}");
}

#[test]
fn unsupported_moves_are_refused() {
    let dir = package(&[
        ("src/shapes.plenty", SHAPES),
        ("src/main.plenty", MAIN),
        ("src/geometry.plenty", "pub class Point:\n    pub z: i64\n"),
    ]);
    let message = refused(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty");
    assert_eq!(message, "`geometry` already declares `Point`");
    let message = refused(dir.path(), "src/shapes.plenty", 4, "src/shapes.plenty");
    assert_eq!(message, "the destination is the declaration's own module");
    let message = refused(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty");
    assert_eq!(message, "this is not a declaration that can move");

    // An import with a comment would lose it when rewritten.
    let dir = package(&[
        ("src/shapes.plenty", SHAPES),
        (
            "src/main.plenty",
            &MAIN.replace("as P\n", "as P  # short\n"),
        ),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let message = refused(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty");
    assert!(message.contains("an import with a comment"), "{message}");

    // `shapes` keeps `norm`, which no target would import afterwards.
    let dir = package(&[
        ("src/shapes.plenty", SHAPES),
        ("src/main.plenty", MAIN),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let message = refused(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty");
    assert_eq!(
        message,
        "after moving `Point`, no target would import src/shapes.plenty"
    );

    // A file outside a package.
    let loose = tempfile::tempdir().unwrap();
    std::fs::write(loose.path().join("a.plenty"), "def f() -> ():\n    pass\n").unwrap();
    std::fs::write(loose.path().join("b.plenty"), "").unwrap();
    let message = refused(loose.path(), "a.plenty", 1, "b.plenty");
    assert!(message.contains("needs a package"), "{message}");
}
