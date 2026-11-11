//! A loop nest reads the header of an owned list, an exclusive reference or
//! a retargetable binding once, before the outermost loop that cannot
//! resize, replace or retarget it, and sibling inner loops share that read;
//! likewise a clist's row count and column addresses (issue #943). A loop
//! that can change the list keeps the read inside it.
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

/// Standard output and error of `source`, which may exit with a failure.
fn run(source: &str, flags: &[&str]) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let mut args = flags.to_vec();
    args.push("main.plenty");
    let output = plenty(dir.path(), &args);
    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// The `ops` dump of `source`, compiled with `--release`.
fn ops(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.plenty"), source).unwrap();
    let output = plenty(
        dir.path(),
        &["--release", "--dump", "ops", "main.plenty", "-o", "out"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_to_string(dir.path().join("out/ops.txt")).unwrap()
}

/// The loop depth of each operation of function `name` whose text starts
/// with `operation`: 0 before the function's outermost loops.
fn depths(dump: &str, name: &str, operation: &str) -> Vec<usize> {
    let start = dump
        .find(&format!("==== {name} "))
        .unwrap_or_else(|| panic!("no section {name} in:\n{dump}"));
    let rest = &dump[start + 5..];
    let section = &rest[..rest.find("\n==== ").unwrap_or(rest.len())];
    let mut loops: Vec<usize> = Vec::new();
    let mut found = Vec::new();
    for line in section.lines() {
        let text = line.trim_start();
        let indent = line.len() - text.len();
        while loops.last().is_some_and(|&at| indent <= at) {
            loops.pop();
        }
        if text == "Loop" {
            loops.push(indent);
        } else if text.starts_with(operation) {
            found.push(loops.len());
        }
    }
    found
}

const LISTS: &str = r#"class Holder:
    items: list[i64]




# Kept: nothing in the outer loop changes either list.
def kept(steps: i64, n: i64) -> Result[i64, AllocError | AppendError[i64] | OverflowError[i64]]:
    mut xs = list[i64].with_capacity(n)?
    mut ys = list[i64].with_capacity(n)?
    for i in range(n):
        xs.append(i)?
        ys.append(0)?
    mut total = 0
    for _ in range(steps):
        for i in range(n):
            ys[i] = xs[i] * 2
        if steps > 0:
            for i in range(n):
                total += ys[i] + xs[i]
    Ok(total)


# Kept across three loops through an exclusive reference.
def deep(xs: &mut list[i64], n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for a in range(2):
        for b in range(2):
            for i in range(n):
                xs[i] += a + b
                total += xs[i]
    Ok(total)


# A field path on an owned class value.
def field(holder: &mut Holder, n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for _ in range(2):
        for i in range(n):
            holder.items[i] += 1
            total += holder.items[i]
    Ok(total)


# The outer loop grows the list: the inner loop reads the header each pass.
def grows(n: i64) -> Result[i64, AllocError | AppendError[i64] | OverflowError[i64]]:
    mut xs = [1]?
    mut total = 0
    for step in range(n):
        m = len(xs)
        for i in range(m):
            total += xs[i]
        xs.append(step + 2)?
    Ok(total)


# The outer loop shrinks the list, so index `n - 1` fails on the second pass.
def shrinks(n: i64) -> Result[i64, AllocError | OverflowError[i64]]:
    mut xs = [1, 2, 3]?
    mut total = 0
    for _ in range(2):
        for i in range(n):
            total += xs[i]
        xs.pop()
    Ok(total)


# The outer loop replaces the binding.
def replaces(n: i64) -> Result[i64, AllocError | OverflowError[i64]]:
    mut xs = [1, 2]?
    mut total = 0
    for step in range(n):
        for i in range(2):
            total += xs[i]
        xs = [step, 10, 20]?
    Ok(total)


# The outer loop retargets the reference.
def retargets(tables: &list[list[i64]]) -> Result[i64, OverflowError[i64]]:
    mut current = &tables[0]
    mut total = 0
    for _ in range(2):
        for i in range(2):
            total += current[i]
        current = &tables[1]
    Ok(total)


def push(xs: &mut list[i64]) -> Result[(), AllocError | AppendError[i64]]:
    xs.append(7)?
    Ok(())


# A call in the outer loop receives the list mutably.
def calls(n: i64) -> Result[i64, AllocError | AppendError[i64] | OverflowError[i64]]:
    mut xs = [1]?
    mut total = 0
    for _ in range(n):
        m = len(xs)
        for i in range(m):
            total += xs[i]
        push(&mut xs)?
    Ok(total)


# A closure called in the outer loop receives the list mutably.
def closure(n: i64) -> Result[i64, AllocError | AppendError[i64] | OverflowError[i64]]:
    mut xs = [1]?
    mut total = 0
    for step in range(n):
        grow = def [step](ys: &mut list[i64]) -> Result[(), AllocError | AppendError[i64]]:
            ys.append(step)?
            Ok(())
        for i in range(len(xs)):
            total += xs[i]
        grow(&mut xs)?
    Ok(total)


# The list is bound inside the outer loop.
def inner_binding(n: i64) -> Result[i64, AllocError | OverflowError[i64]]:
    mut total = 0
    for step in range(n):
        xs = [step, step + 1]?
        for i in range(2):
            total += xs[i]
    Ok(total)


# The length is the inner loop's range stop.
def stop(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for _ in range(2):
        for i in range(len(xs)):
            xs[i] += 1
            total += xs[i]
    Ok(total)


# The middle loop keeps the list; the outer one resizes it.
def middle(n: i64) -> Result[i64, AllocError | AppendError[i64] | OverflowError[i64]]:
    mut xs = [1]?
    mut total = 0
    for step in range(n):
        m = len(xs)
        for _ in range(2):
            for i in range(m):
                total += xs[i]
        xs.append(step)?
    Ok(total)
"#;

const LISTS_MAIN: &str = r#"
def main() -> Result[(), AllocError | AppendError[i64] | IoError | OverflowError[i64]]:
    mut xs = [1, 2, 3]?
    mut holder = Holder([4, 5]?)
    tables = [[1, 2]?, [30, 40, 50]?]?
    print(kept(3, 4)?, deep(&mut xs, 3)?, field(&mut holder, 2)?)?
    print(grows(4)?, replaces(3)?, retargets(&tables)?)?
    print(calls(3)?, closure(3)?, inner_binding(3)?, stop(&mut xs)?, middle(3)?)?
    print(shrinks(2)?)?
    print(shrinks(3)?)?
    Ok(())
"#;

/// The row address read of each list a function indexes, by loop depth.
#[test]
fn list_headers_move_out_of_loops_that_cannot_change_the_list() {
    let dump = ops(&format!("{LISTS}\ndef main() -> ():\n    ()\n"));
    let rows = |name| depths(&dump, name, "Collection(ListRowsRef(");
    // `xs` and `ys`, each read once for both inner loops.
    assert_eq!(rows("kept"), [0, 0]);
    assert_eq!(depths(&dump, "kept", "Collection(ListLenRef("), [0, 0]);
    assert_eq!(rows("deep"), [0]);
    assert_eq!(rows("field"), [0]);
    // Every access still uses the cache.
    for name in ["kept", "deep", "field", "grows", "shrinks", "middle"] {
        assert!(
            depths(&dump, name, "Collection(ListScalarGetRef(").is_empty(),
            "{name}"
        );
    }
    for name in [
        "grows",
        "shrinks",
        "replaces",
        "calls",
        "closure",
        "inner_binding",
        "stop",
        "middle",
    ] {
        assert_eq!(rows(name), [1], "{name}");
    }
    // `tables`, a shared reference, then `current` inside the outer loop.
    assert_eq!(rows("retargets"), [0, 1]);
}

/// The loops that change their list between inner passes see each change,
/// in both build modes: a shrunk list traps at the removed row.
#[test]
fn hoisted_list_headers_keep_results_and_traps() {
    let source = format!("{LISTS}{LISTS_MAIN}");
    for flags in [&["--release"][..], &["--fast-compile"]] {
        let (out, err) = run(&source, flags);
        assert_eq!(out, "54 45 24\n20 24 73\n24 4 9 45 8\n6\n", "{flags:?}");
        assert_eq!(
            err, "error: main.plenty:63:22: index out of bounds: index 2, length 2\n",
            "{flags:?}"
        );
    }
}

const CLISTS: &str = r#"@copy
class Point:
    x: i64
    y: i64


class World:
    points: clist[Point]


# Kept: an exclusive clist that nothing in the outer loop resizes.
def kept(points: &mut clist[Point], n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for step in range(3):
        for i in range(n):
            points[i].x += step
        for i in range(n):
            total += points[i].x + points[i].y
    Ok(total)


# A shared clist indexed by the outer and the inner loop.
def shared(points: &clist[Point], n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for i in range(n):
        total += points[i].y
        for j in range(n):
            total += points[i].x * points[j].y
    Ok(total)


# An owned clist behind a field path.
def field(world: &mut World, n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for _ in range(2):
        for i in range(n):
            world.points[i].y += 1
            total += world.points[i].y
    Ok(total)


# The outer loop appends a row.
def grows(n: i64) -> Result[i64, AllocError | AppendError[Point] | OverflowError[i64]]:
    mut points = clist[Point].with_capacity(1)?
    points.append(Point(1, 1))?
    mut total = 0
    for step in range(n):
        m = len(points)
        for i in range(m):
            total += points[i].x
        points.append(Point(step + 2, 0))?
    Ok(total)


# The outer loop clears the clist, so the second pass fails at row 0.
def clears(points: &mut clist[Point], n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for _ in range(n):
        for i in range(2):
            total += points[i].x
        points.clear()
    Ok(total)


def push(points: &mut clist[Point]) -> Result[(), AllocError | AppendError[Point]]:
    points.append(Point(7, 7))?
    Ok(())


# A call in the outer loop receives the clist mutably.
def calls(n: i64) -> Result[i64, AllocError | AppendError[Point] | OverflowError[i64]]:
    mut points = clist[Point].with_capacity(1)?
    points.append(Point(1, 1))?
    mut total = 0
    for _ in range(n):
        m = len(points)
        for i in range(m):
            total += points[i].x
        push(&mut points)?
    Ok(total)


# The row count is the inner loop's range stop; the column is not.
def stop(points: &mut clist[Point]) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for _ in range(2):
        for i in range(len(points)):
            points[i].x += 1
            total += points[i].x
    Ok(total)
"#;

const CLISTS_MAIN: &str = r#"
def main() -> Result[(), AllocError | AppendError[Point] | IoError | OverflowError[i64]]:
    mut points = clist[Point].with_capacity(3)?
    for i in range(3):
        points.append(Point(i, i * 10))?
    mut world = World(clist[Point].with_capacity(2)?)
    world.points.append(Point(1, 2))?
    world.points.append(Point(3, 4))?
    print(kept(&mut points, 3)?, shared(&points, 3)?, field(&mut world, 2)?)?
    print(grows(4)?, calls(3)?, stop(&mut points)?)?
    print(clears(&mut points, 1)?)?
    print(clears(&mut world.points, 2)?)?
    Ok(())
"#;

/// The row count and column address reads of each clist, by loop depth.
#[test]
fn clist_headers_move_out_of_loops_that_cannot_change_the_clist() {
    let dump = ops(&format!("{CLISTS}\ndef main() -> ():\n    ()\n"));
    let columns = |name| depths(&dump, name, "Collection(Clist(Column(");
    let counts = |name| depths(&dump, name, "Collection(Clist(Len(");
    // Both columns once for the two inner loops.
    assert_eq!(columns("kept"), [0, 0]);
    assert_eq!(counts("kept"), [0]);
    // A shared reference: the outer and the inner loop share each column.
    assert_eq!(columns("shared"), [0, 0]);
    assert_eq!(counts("shared"), [0]);
    assert_eq!(columns("field"), [0]);
    assert_eq!(counts("field"), [0]);
    for name in ["grows", "clears", "calls"] {
        assert_eq!(columns(name), [1], "{name}");
        assert!(counts(name).iter().all(|&depth| depth == 1), "{name}");
    }
    // The count is the inner loop's stop; the column does not depend on it.
    assert_eq!(columns("stop"), [0]);
    assert_eq!(counts("stop"), [1]);
    for name in [
        "kept", "shared", "field", "grows", "clears", "calls", "stop",
    ] {
        assert!(
            depths(&dump, name, "Collection(Clist(Get(").is_empty(),
            "{name}"
        );
    }
}

#[test]
fn hoisted_clist_headers_keep_results_and_traps() {
    let source = format!("{CLISTS}{CLISTS_MAIN}");
    for flags in [&["--release"][..], &["--fast-compile"]] {
        let (out, err) = run(&source, flags);
        assert_eq!(out, "111 390 18\n20 24 33\n11\n", "{flags:?}");
        assert_eq!(
            err, "error: main.plenty:60:22: clist index out of bounds\n",
            "{flags:?}"
        );
    }
}
