//! Explicit list element references (issue #378): `&xs[i]` and `&mut xs[i]`
//! take the row address from the narrow `plenty_list_row` helper instead of
//! the generic collection dispatcher. References keep their semantics: loans,
//! writes through `&mut`, returning an element, nested and non-scalar rows,
//! and the same out-of-bounds diagnostic and site.
mod support;

use std::path::Path;
use std::process::{Command, Output};

const PROGRAM: &str = r#"class Point:
    x: i64
    y: i64

def bump(xs: &mut list[i64], i: i64) -> ():
    r = &mut xs[i]
    *r = *r + 1

def pick(xs: &list[str], i: i64) -> &str:
    &xs[i]

def rename(xs: &mut list[str], i: i64, name: str) -> ():
    r = &mut xs[i]
    *r = name

def shift(points: &mut list[Point], i: i64) -> ():
    p = &mut points[i]
    p.x = p.x + p.y

def toggle(flags: &mut list[bool], i: i64) -> ():
    f = &mut flags[i]
    *f = not *f

def wrap(bytes: &mut list[u8], i: i64) -> ():
    b = &mut bytes[i]
    *b = b.wrapping_add(10u8)

def halve(values: &mut list[f32], i: i64) -> ():
    v = &mut values[i]
    *v = *v / 2.0f32

def scale(grid: &mut list[list[i64]], i: i64, j: i64) -> ():
    c = &mut grid[i][j]
    *c = *c * 100

def element(xs: &list[i64], i: i64) -> &i64:
    &xs[i]

def read(value: &i64) -> i64:
    *value

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3]?
    bump(&mut xs, 0)
    bump(&mut xs, -1)
    print(xs, read(element(&xs, 1)), *element(&xs, -3))?
    r = &xs[2]
    print(*r)?
    xs.append(5)?
    print(xs)?

    mut names = ["a", "b"]?
    print(pick(&names, 0), pick(&names, -1))?
    rename(&mut names, 1, "cde")
    print(names, len(*pick(&names, 1)))?

    mut points = [Point(1, 2), Point(3, 4)]?
    shift(&mut points, -1)
    print(points[0].x, points[1].x)?

    mut flags = [True, False]?
    toggle(&mut flags, 1)
    print(flags)?
    mut bytes = [1u8, 250u8]?
    wrap(&mut bytes, 1)
    print(bytes)?
    mut halves = [1.0f32, 3.0f32]?
    halve(&mut halves, 1)
    print(halves)?

    # Growing an inner list does not move the outer list's rows.
    mut grid = [[1, 2]?, [3]?]?
    scale(&mut grid, 0, 1)
    scale(&mut grid, -1, -1)
    inner = &mut grid[1]
    for i in range(100):
        inner.append(i)?
    print(grid[0], len(grid[1]), grid[1][0])?
    Ok(())
"#;

const EXPECTED: &str = r#"[2, 2, 4] 2 2
4
[2, 2, 4, 5]
a b
["a", "cde"] 3
1 7
[True, True]
[1, 4]
[1.0, 1.5]
[1, 200] 101 300
"#;

fn plenty(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plenty"))
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

fn stdout(dir: &Path, args: &[&str]) -> String {
    let output = plenty(dir, args);
    assert!(
        output.status.success(),
        "plenty {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn setup(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("refs.plenty"), source).unwrap();
    dir
}

#[test]
fn element_references_keep_reference_semantics() {
    let dir = setup(PROGRAM);
    assert_eq!(stdout(dir.path(), &["refs.plenty"]), EXPECTED);
    assert_eq!(stdout(dir.path(), &["--release", "refs.plenty"]), EXPECTED);
}

/// The generic dispatcher reported these messages and sites; the narrow
/// helper must report the same ones.
#[test]
fn out_of_bounds_references_keep_the_diagnostic_and_site() {
    let cases = [
        (
            "def pick(xs: &list[str], i: i64) -> &str:\n    &xs[i]\n\ndef main() -> Result[(), AllocError | IoError]:\n    names = [\"a\", \"b\"]?\n    print(pick(&names, -2))?\n    print(pick(&names, 2))?\n    Ok(())\n",
            "a\n",
            "error: refs.plenty:2:6: index out of bounds: index 2, length 2\n",
        ),
        (
            "def main() -> Result[(), AllocError | IoError]:\n    mut grid = [[1, 2]?, [3]?]?\n    r = &mut grid[0][1]\n    *r = 9\n    print(grid)?\n    q = &mut grid[1][-2]\n    *q = 1\n    Ok(())\n",
            "[[1, 9], [3]]\n",
            "error: refs.plenty:6:14: index out of bounds: index -2, length 1\n",
        ),
        (
            "def main() -> Result[(), AllocError | IoError]:\n    empty: list[i64] = []?\n    r = &empty[0]\n    print(*r)?\n    Ok(())\n",
            "",
            "error: refs.plenty:3:10: index out of bounds: index 0, length 0\n",
        ),
    ];
    for (source, expected_stdout, expected_stderr) in cases {
        let dir = setup(source);
        for args in [&["refs.plenty"][..], &["--release", "refs.plenty"]] {
            let output = plenty(dir.path(), args);
            assert!(!output.status.success(), "{source}");
            assert_eq!(String::from_utf8_lossy(&output.stdout), expected_stdout);
            assert_eq!(String::from_utf8_lossy(&output.stderr), expected_stderr);
        }
    }
}

#[test]
fn element_references_hold_the_list_loan() {
    let shared = support::check_error(
        "mut xs = [1, 2].unwrap()\nr = &xs[0]\nxs.append(3).unwrap()\nprint(*r).unwrap()\n",
    );
    assert!(shared.contains("while `xs[0]` is borrowed"), "{shared}");
    let exclusive = support::check_error(
        "mut xs = [1, 2].unwrap()\nr = &mut xs[0]\nprint(len(xs)).unwrap()\n*r = 3\n",
    );
    assert!(
        exclusive.contains("while `xs[0]` is exclusively borrowed"),
        "{exclusive}"
    );
}

/// Explicit element references call the narrow helper, not the dispatcher.
#[test]
fn element_references_use_the_narrow_row_helper() {
    let dir = setup(PROGRAM);
    for function in ["bump", "pick", "rename", "shift", "scale", "element"] {
        let asm = stdout(
            dir.path(),
            &[
                "--release",
                "--dump",
                "asm",
                "--dump-filter",
                function,
                "refs.plenty",
                "-o",
                "-",
            ],
        );
        assert!(asm.contains("# plenty_list_row\n"), "{function}: {asm}");
        assert!(!asm.contains("plenty_op_"), "{function}: {asm}");
    }
}
