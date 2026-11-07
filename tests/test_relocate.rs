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

/// Every source file below `dir`, by relative path.
fn snapshot(dir: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                // `plenty run` writes its build output here.
                if path != root.join("target") {
                    walk(root, &path, out);
                }
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
            "moves public `tools.shapes.Point` to `tools.geometry.Point`; packages outside this workspace that import it must change",
            "changes the printed type name `shapes.Point` to `geometry.Point`",
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
        ["makes `geometry.double` public, because other modules use it"]
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
    assert_eq!(
        message,
        "fix this error before moving `two`: src/shapes.plenty:5:5: arithmetic can fail with OverflowError[i64], which is not included in this function's return type i64; declare Result[_, OverflowError[i64]], handle a local `checked:` result, or deliberately use `unchecked:`"
    );

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
            "moves public `tools.shapes.Point` to `tools.geometry.Point`; packages outside this workspace that import it must change",
            "changes the printed type name `shapes.Point` to `geometry.Point`",
        ]
    );
    assert_eq!(
        moved.files["src/lib.plenty"],
        "import geometry\n\npub def origin() -> geometry.Point:\n    geometry.Point(x=0, y=0)\n"
    );
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
    assert_eq!(
        message,
        "moving `twice` would make `shapes` and `geometry` import each other: `twice` uses `double` and the rest of `shapes` uses `twice`"
    );

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
    let src = dir.path().canonicalize().unwrap().join("src");
    let (geometry, shapes) = (
        src.join("geometry.plenty").display().to_string(),
        src.join("shapes.plenty").display().to_string(),
    );
    assert_eq!(
        message,
        format!(
            "after moving `one`, the package would not check: src/main.plenty:1:1: {geometry}:1:1: {shapes}:1:1: circular import: {geometry} -> {shapes} -> {geometry}"
        )
    );
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
    assert_eq!(
        message,
        "src/main.plenty:2: an import with a comment cannot be rewritten"
    );

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
    assert_eq!(
        message,
        "moving a declaration needs a package: this file has no plenty.toml"
    );
}

/// Write a planned move's files into the package.
fn write_moved(dir: &Path, moved: &Moved) {
    for (path, text) in &moved.files {
        std::fs::write(dir.join(path), text).unwrap();
    }
}

/// The standard output of `plenty run` in the package.
fn run(dir: &Path) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("run")
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

const PRINTING_MAIN: &str = "\
import shapes

def main() -> Result[(), AllocError | IoError]:
    print(shapes.TEXT)?
    Ok(())
";

#[test]
fn an_included_file_moves_only_within_its_directory() {
    let files = [
        (
            "src/shapes.plenty",
            "pub const TEXT: str = include_str(\"data.txt\")\n",
        ),
        ("src/data.txt", "included"),
        ("src/main.plenty", PRINTING_MAIN),
        ("src/geometry.plenty", GEOMETRY),
        ("src/sub/geometry.plenty", GEOMETRY),
    ];
    let dir = package(&files);
    let message = refused(
        dir.path(),
        "src/shapes.plenty",
        1,
        "src/sub/geometry.plenty",
    );
    assert_eq!(
        message,
        "`TEXT` includes a file by a path relative to src/; move it to a module in that directory, or change the path first"
    );

    assert_eq!(run(dir.path()), "included\n");
    let moved = plan(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.files,
        BTreeMap::from([
            (
                "src/geometry.plenty".to_string(),
                "pub def unit() -> i64:\n    1\n\npub const TEXT: str = include_str(\"data.txt\")\n"
                    .to_string()
            ),
            ("src/shapes.plenty".to_string(), String::new()),
            (
                "src/main.plenty".to_string(),
                PRINTING_MAIN.replace("shapes", "geometry")
            ),
        ])
    );
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), "included\n");
}

#[test]
fn a_module_whose_path_ends_with_the_source_is_left_alone() {
    let net = "pub const TEXT: str = \"net\"\n";
    let main = "\
import shapes
import net.shapes
from net.shapes import TEXT

def main() -> Result[(), AllocError | IoError]:
    print(shapes.TEXT)?
    print(net.shapes.TEXT)?
    print(TEXT)?
    Ok(())
";
    let dir = package(&[
        ("src/shapes.plenty", "pub const TEXT: str = \"local\"\n"),
        ("src/net/shapes.plenty", net),
        ("src/main.plenty", main),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    assert_eq!(run(dir.path()), "local\nnet\nnet\n");
    let moved = plan(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.files["src/main.plenty"],
        main.replacen("import shapes\n", "", 1)
            .replacen("TEXT\n\n", "TEXT\nimport geometry\n\n", 1)
            .replace("print(shapes.TEXT)", "print(geometry.TEXT)")
    );
    assert!(!moved.files.contains_key("src/net/shapes.plenty"));
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), "local\nnet\nnet\n");
}

#[test]
fn import_words_in_strings_and_docstrings_are_not_imports() {
    let shapes = "\
import std.math as m

pub const NOTE: str = \"\"\"
import geometry
from shapes import NOTE
\"\"\"

pub def norm(x: f64) -> f64:
    \"\"\"The distance,
    from the start.\"\"\"
    m.fabs(x)
";
    let main = "\
import shapes

def main() -> Result[(), AllocError | IoError]:
    print(shapes.norm(-2.0))?
    print(shapes.NOTE)?
    Ok(())
";
    let dir = package(&[
        ("src/shapes.plenty", shapes),
        ("src/main.plenty", main),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let before = run(dir.path());
    assert_eq!(
        before,
        "2.0\n\nimport geometry\nfrom shapes import NOTE\n\n"
    );
    let moved = plan(dir.path(), "src/shapes.plenty", 8, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.files,
        BTreeMap::from([
            (
                "src/geometry.plenty".to_string(),
                "import std.math as m\n\npub def unit() -> i64:\n    1\n\npub def norm(x: f64) -> f64:\n    \"\"\"The distance,\n    from the start.\"\"\"\n    m.fabs(x)\n".to_string()
            ),
            (
                "src/shapes.plenty".to_string(),
                "pub const NOTE: str = \"\"\"\nimport geometry\nfrom shapes import NOTE\n\"\"\"\n".to_string()
            ),
            (
                "src/main.plenty".to_string(),
                "import shapes\nimport geometry\n\ndef main() -> Result[(), AllocError | IoError]:\n    print(geometry.norm(-2.0))?\n    print(shapes.NOTE)?\n    Ok(())\n".to_string()
            ),
        ])
    );
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), before);
}

#[test]
fn std_imports_are_matched_by_module_and_combined() {
    let shapes = "\
import std.math
from std.math import fabs, degrees

pub def norm(x: f64) -> (f64, f64):
    (degrees(fabs(x)), std.math.radians(x))

pub def keep(x: f64) -> f64:
    fabs(x)
";
    let geometry = "import std.path\nfrom std.math import fabs\n\npub def unit() -> i64:\n    1\n";
    let main = "\
import shapes

def main() -> Result[(), AllocError | IoError]:
    print(shapes.norm(-4.0))?
    print(shapes.keep(-1.0))?
    Ok(())
";
    let dir = package(&[
        ("src/shapes.plenty", shapes),
        ("src/main.plenty", main),
        ("src/geometry.plenty", geometry),
    ]);
    let before = run(dir.path());
    let moved = plan(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.files["src/geometry.plenty"],
        "import std.path\nfrom std.math import fabs\nimport std.math\nfrom std.math import degrees\n\npub def unit() -> i64:\n    1\n\npub def norm(x: f64) -> (f64, f64):\n    (degrees(fabs(x)), std.math.radians(x))\n"
    );
    assert_eq!(
        moved.files["src/shapes.plenty"],
        "from std.math import fabs\n\npub def keep(x: f64) -> f64:\n    fabs(x)\n"
    );
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), before);
}

#[test]
fn a_crlf_destination_keeps_its_line_endings() {
    let dir = package(&[
        (
            "src/shapes.plenty",
            "import std.math as m\nfrom std.math import degrees\n\npub def norm(x: f64) -> f64:\n    degrees(m.fabs(x))\n",
        ),
        (
            "src/main.plenty",
            "import shapes\n\ndef main() -> Result[(), AllocError | IoError]:\n    print(shapes.norm(-4.0))?\n    Ok(())\n",
        ),
        ("src/geometry.plenty", "pub def unit() -> i64:\r\n    1\r\n"),
    ]);
    let before = run(dir.path());
    let moved = plan(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.files["src/geometry.plenty"],
        "import std.math as m\r\nfrom std.math import degrees\r\n\r\npub def unit() -> i64:\r\n    1\r\n\r\npub def norm(x: f64) -> f64:\r\n    degrees(m.fabs(x))\r\n"
    );
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), before);
}

#[test]
fn documented_refusals() {
    let dir = package(&[
        ("src/shapes.plenty", SHAPES),
        ("src/main.plenty", MAIN),
        ("src/geometry.plenty", GEOMETRY),
        ("tests/points.plenty", TEST),
    ]);
    assert_eq!(
        refused(dir.path(), "src/shapes.plenty", 4, "tests/points.plenty"),
        "the destination must be a module of the same package's src/"
    );

    for (text, keyword) in [
        ("opaque Handle\n", "opaque"),
        (
            "extern def puts(text: &str as utf8) -> i32 = \"puts\"\n",
            "extern",
        ),
        ("dimension Speed = Length / Time\n", "dimension"),
        (
            "if const target.os == \"linux\":\n    const A: i64 = 1\n",
            "if",
        ),
    ] {
        let dir = package(&[
            ("src/shapes.plenty", text),
            ("src/main.plenty", "def main() -> ():\n    pass\n"),
            ("src/geometry.plenty", GEOMETRY),
        ]);
        assert_eq!(
            refused(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty"),
            format!("moving `{keyword}` declarations is not supported")
        );
    }

    // The destination imports another `Point`.
    let dir = package(&[
        ("src/shapes.plenty", SHAPES),
        (
            "src/main.plenty",
            &MAIN.replace("    show(a)\n", "    n = shapes.norm(&a)\n    show(a)\n"),
        ),
        ("src/other.plenty", "pub class Point:\n    pub z: i64\n"),
        (
            "src/geometry.plenty",
            "from other import Point\n\npub def unit() -> i64:\n    1\n",
        ),
    ]);
    assert_eq!(
        refused(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty"),
        "`geometry` already imports a `Point`"
    );

    // The moved code's `m` and the destination's `m` are different modules.
    let dir = package(&[
        ("src/shapes.plenty", SHAPES),
        (
            "src/main.plenty",
            "import shapes\n\ndef main() -> ():\n    x = shapes.norm(&shapes.Point(x=1, y=2))\n",
        ),
        (
            "src/geometry.plenty",
            "import std.path as m\n\npub def unit() -> i64:\n    1\n",
        ),
    ]);
    assert_eq!(
        refused(dir.path(), "src/shapes.plenty", 8, "src/geometry.plenty"),
        "`norm` needs `import std.math as m`, but `geometry` already has `import std.path as m`"
    );

    // An import inside `if const` cannot be rewritten.
    let dir = package(&[
        ("src/shapes.plenty", "pub def one() -> i64:\n    1\n"),
        (
            "src/main.plenty",
            "if const target.os == \"linux\":\n    import shapes\n\ndef main() -> ():\n    x = shapes.one()\n",
        ),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let message = refused(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty");
    assert_eq!(
        message,
        "src/main.plenty: line 2 has an indented import, which the move cannot rewrite"
    );

    // The moved code calls a `unit` that is not the destination's.
    let dir = package(&[
        (
            "src/shapes.plenty",
            "from other import unit\n\npub def two() -> i64:\n    unit()\n",
        ),
        ("src/other.plenty", "pub def unit() -> i64:\n    2\n"),
        (
            "src/main.plenty",
            "import shapes\n\ndef main() -> ():\n    x = shapes.two()\n",
        ),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    assert_eq!(
        refused(dir.path(), "src/shapes.plenty", 3, "src/geometry.plenty"),
        "`two` uses `unit`, which `geometry` declares as something else"
    );

    // A member access spaced or split across lines is not rewritten.
    for (access, at) in [("shapes . ONE", "4:18"), ("(shapes\n        .ONE)", "5:10")] {
        let dir = package(&[
            ("src/shapes.plenty", "pub const ONE: i64 = 1\n"),
            (
                "src/main.plenty",
                &format!("import shapes\n\ndef main() -> ():\n    x = {access}\n"),
            ),
            ("src/geometry.plenty", GEOMETRY),
        ]);
        let message = refused(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty");
        assert_eq!(
            message,
            format!("src/main.plenty:{at}: cannot rewrite this reference to `ONE`")
        );
    }
}

#[test]
fn the_destinations_import_of_the_moved_declaration_is_removed() {
    let geometry = "from shapes import ONE as one\n\npub def two() -> i64:\n    one\n";
    let dir = package(&[
        ("src/shapes.plenty", "pub const ONE: i64 = 1\n"),
        ("src/geometry.plenty", geometry),
        (
            "src/main.plenty",
            "import geometry\n\ndef main() -> Result[(), AllocError | IoError]:\n    print(geometry.two())?\n    Ok(())\n",
        ),
    ]);
    assert_eq!(run(dir.path()), "1\n");
    let moved = plan(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.files,
        BTreeMap::from([
            (
                "src/geometry.plenty".to_string(),
                "pub def two() -> i64:\n    ONE\n\npub const ONE: i64 = 1\n".to_string()
            ),
            ("src/shapes.plenty".to_string(), String::new()),
        ])
    );
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), "1\n");
}

const DECORATED: &str = "\
pub def unit() -> i64:
    1

# Hidden from printing.
@redacted
pub class Secret:
    pub code: i64

@copy
pub class Pair:
    pub a: i64
";

const DECORATED_MAIN: &str = "\
import shapes

def main() -> Result[(), AllocError | IoError]:
    print(shapes.Secret(code=7))?
    p = shapes.Pair(a=2)
    q = p
    print(p.a)?
    print(q.a)?
    print(shapes.unit())?
    Ok(())
";

#[test]
fn decorators_and_their_comments_move_with_the_declaration() {
    let dir = package(&[
        ("src/shapes.plenty", DECORATED),
        ("src/main.plenty", DECORATED_MAIN),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let before = run(dir.path());
    assert_eq!(before, "shapes.Secret(@redacted)\n2\n2\n1\n");
    // The action is offered on the comment and decorator lines too.
    let path = dir.path().join("src/shapes.plenty");
    for line in [4, 5, 6, 7] {
        let candidate = relocate::candidates(&path, DECORATED, line).unwrap();
        assert_eq!(candidate.name, "Secret", "line {line}");
    }
    assert_eq!(
        relocate::candidates(&path, DECORATED, 9).unwrap().name,
        "Pair"
    );
    let moved = plan(dir.path(), "src/shapes.plenty", 5, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.api_changes,
        ["changes the printed type name `shapes.Secret` to `geometry.Secret`"]
    );
    let expected = BTreeMap::from([
        (
            "src/geometry.plenty".to_string(),
            "pub def unit() -> i64:\n    1\n\n# Hidden from printing.\n@redacted\npub class Secret:\n    pub code: i64\n".to_string(),
        ),
        (
            "src/shapes.plenty".to_string(),
            "pub def unit() -> i64:\n    1\n\n@copy\npub class Pair:\n    pub a: i64\n".to_string(),
        ),
        (
            "src/main.plenty".to_string(),
            DECORATED_MAIN
                .replace("import shapes\n", "import shapes\nimport geometry\n")
                .replace("shapes.Secret", "geometry.Secret"),
        ),
    ]);
    assert_eq!(moved.files, expected);
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), "geometry.Secret(@redacted)\n2\n2\n1\n");

    // `@copy` moves with `Pair`: `q = p` still copies.
    let moved = plan(dir.path(), "src/shapes.plenty", 4, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.files["src/shapes.plenty"],
        "pub def unit() -> i64:\n    1\n"
    );
    assert_eq!(
        moved.files["src/geometry.plenty"],
        "pub def unit() -> i64:\n    1\n\n# Hidden from printing.\n@redacted\npub class Secret:\n    pub code: i64\n\n@copy\npub class Pair:\n    pub a: i64\n"
    );
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), "geometry.Secret(@redacted)\n2\n2\n1\n");
}

const COMMENT_IN_STRING: &str = "\
pub const BANNER: str = \"\"\"top
# not a comment
\"\"\"
# Belongs to `after`.
pub def after() -> i64:
    2
";

#[test]
fn a_hash_line_inside_a_multi_line_string_stays_with_the_string() {
    let dir = package(&[
        ("src/shapes.plenty", COMMENT_IN_STRING),
        (
            "src/main.plenty",
            "import shapes\n\ndef main() -> Result[(), AllocError | IoError]:\n    print(shapes.BANNER)?\n    print(shapes.after())?\n    Ok(())\n",
        ),
        ("src/geometry.plenty", GEOMETRY),
    ]);
    let before = run(dir.path());
    assert_eq!(before, "top\n# not a comment\n\n2\n");
    let path = dir.path().join("src/shapes.plenty");
    assert_eq!(
        relocate::candidates(&path, COMMENT_IN_STRING, 2)
            .unwrap()
            .name,
        "BANNER"
    );
    let moved = plan(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty").unwrap();
    assert_eq!(
        moved.files["src/geometry.plenty"],
        "pub def unit() -> i64:\n    1\n\npub const BANNER: str = \"\"\"top\n# not a comment\n\"\"\"\n"
    );
    assert_eq!(
        moved.files["src/shapes.plenty"],
        "# Belongs to `after`.\npub def after() -> i64:\n    2\n"
    );
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), before);
}

#[test]
fn line_breaks_inside_a_multi_line_string_keep_their_bytes() {
    let main = "import shapes\n\ndef main() -> Result[(), AllocError | IoError]:\n    print(shapes.TEXT.byte_len())?\n    print(shapes.TEXT)?\n    Ok(())\n";
    // A CRLF source into an LF destination, and an LF source into a CRLF one.
    for (source, destination, expected) in [
        (
            "pub const TEXT: str = \"\"\"a\r\nb\r\n\"\"\"\r\n",
            "pub def unit() -> i64:\n    1\n",
            "pub def unit() -> i64:\n    1\n\npub const TEXT: str = \"\"\"a\r\nb\r\n\"\"\"\n",
        ),
        (
            "pub const TEXT: str = \"\"\"a\nb\n\"\"\"\n",
            "pub def unit() -> i64:\r\n    1\r\n",
            "pub def unit() -> i64:\r\n    1\r\n\r\npub const TEXT: str = \"\"\"a\nb\n\"\"\"\r\n",
        ),
    ] {
        let dir = package(&[
            ("src/shapes.plenty", source),
            ("src/main.plenty", main),
            ("src/geometry.plenty", destination),
        ]);
        let before = run(dir.path());
        let printed = if source.contains('\r') {
            "6\na\r\nb\r\n\n"
        } else {
            "4\na\nb\n\n"
        };
        assert_eq!(before, printed);
        let moved = plan(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty").unwrap();
        assert_eq!(moved.files["src/geometry.plenty"], expected);
        write_moved(dir.path(), &moved);
        assert_eq!(run(dir.path()), before);
    }
}

const PRINT_MAIN_PREFIX: &str = "def main() -> Result[(), AllocError | IoError]:\n";

#[test]
fn a_rewritten_name_captured_by_a_parameter_is_refused() {
    // (a) In the moved text, `geometry.SCALE` would become `SCALE`, the
    // parameter.
    let dir = package(&[
        (
            "src/shapes.plenty",
            "import geometry\n\npub def first(SCALE: i64) -> i64:\n    geometry.SCALE\n",
        ),
        ("src/geometry.plenty", "pub const SCALE: i64 = 10\n"),
        (
            "src/main.plenty",
            &format!(
                "import shapes\n\n{PRINT_MAIN_PREFIX}    print(shapes.first(1))?\n    Ok(())\n"
            ),
        ),
    ]);
    assert_eq!(run(dir.path()), "10\n");
    assert_eq!(
        refused(dir.path(), "src/shapes.plenty", 3, "src/geometry.plenty"),
        "src/shapes.plenty:4:5: after the move, `geometry.SCALE` in `first` would name something else"
    );

    // (b) In the destination, `shapes.LIMIT` would become `LIMIT`, the
    // parameter.
    let dir = package(&[
        ("src/shapes.plenty", "pub const LIMIT: i64 = 5\n"),
        (
            "src/geometry.plenty",
            "import shapes\n\npub def limit(LIMIT: i64) -> i64:\n    shapes.LIMIT\n",
        ),
        (
            "src/main.plenty",
            &format!(
                "import geometry\n\n{PRINT_MAIN_PREFIX}    print(geometry.limit(1))?\n    Ok(())\n"
            ),
        ),
    ]);
    assert_eq!(run(dir.path()), "5\n");
    assert_eq!(
        refused(dir.path(), "src/shapes.plenty", 1, "src/geometry.plenty"),
        "src/geometry.plenty:4:5: after the move, `shapes.LIMIT` would no longer name `LIMIT`"
    );

    // (c) In another caller, `shapes.size()` would become `config.size()`,
    // the method of the parameter `config`.
    let dir = package(&[
        ("src/shapes.plenty", "pub def size() -> i64:\n    3\n"),
        ("src/config.plenty", GEOMETRY),
        (
            "src/main.plenty",
            &format!(
                "import shapes\n\nclass Crate:\n    pub n: i64\n\n    def size(self) -> i64:\n        self.n\n\ndef measure(config: Crate) -> i64:\n    shapes.size()\n\n{PRINT_MAIN_PREFIX}    print(measure(Crate(n=9)))?\n    Ok(())\n"
            ),
        ),
    ]);
    assert_eq!(run(dir.path()), "3\n");
    assert_eq!(
        refused(dir.path(), "src/shapes.plenty", 1, "src/config.plenty"),
        "src/main.plenty:10:5: after the move, `shapes.size` would no longer name `size`"
    );
}

#[test]
fn moving_into_the_library_imports_it_under_the_package_name() {
    let dir = package(&[
        ("src/lib.plenty", "pub def version() -> i64:\n    1\n"),
        (
            "src/shapes.plenty",
            "pub class Point:\n    pub x: i64\n\npub def unit() -> i64:\n    1\n",
        ),
        (
            "src/main.plenty",
            &format!(
                "import shapes\nfrom shapes import Point\n\n{PRINT_MAIN_PREFIX}    print(shapes.Point(x=1))?\n    print(Point(x=2))?\n    print(shapes.unit())?\n    Ok(())\n"
            ),
        ),
        (
            "tests/points.plenty",
            "import tools.shapes\nfrom tools.shapes import Point\n\ndef test_point() -> ():\n    p = tools.shapes.Point(x=1)\n    q = Point(x=2)\n",
        ),
    ]);
    assert_eq!(run(dir.path()), "shapes.Point(x=1)\nshapes.Point(x=2)\n1\n");
    let moved = plan(dir.path(), "src/shapes.plenty", 1, "src/lib.plenty").unwrap();
    assert_eq!(
        moved.api_changes,
        [
            "moves public `tools.shapes.Point` to `tools.Point`; packages outside this workspace that import it must change",
            "changes the printed type name `shapes.Point` to `lib.Point`",
        ]
    );
    let expected = BTreeMap::from([
        (
            "src/lib.plenty".to_string(),
            "pub def version() -> i64:\n    1\n\npub class Point:\n    pub x: i64\n".to_string(),
        ),
        (
            "src/shapes.plenty".to_string(),
            "pub def unit() -> i64:\n    1\n".to_string(),
        ),
        (
            "src/main.plenty".to_string(),
            format!(
                "import shapes\nfrom lib import Point\nimport lib\n\n{PRINT_MAIN_PREFIX}    print(lib.Point(x=1))?\n    print(Point(x=2))?\n    print(shapes.unit())?\n    Ok(())\n"
            ),
        ),
        (
            "tests/points.plenty".to_string(),
            "from tools import Point\nimport tools\n\ndef test_point() -> ():\n    p = tools.Point(x=1)\n    q = Point(x=2)\n".to_string(),
        ),
    ]);
    assert_eq!(moved.files, expected);
    write_moved(dir.path(), &moved);
    assert_eq!(run(dir.path()), "lib.Point(x=1)\nlib.Point(x=2)\n1\n");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("test")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        refused(dir.path(), "src/lib.plenty", 1, "src/shapes.plenty"),
        "`lib` is the package's library, which tests and other packages import under the package name; moving declarations out of it is not supported"
    );
}
