use super::*;

fn function<'a>(ops: &'a [Op], name: &str) -> &'a CompiledFn {
    ops.iter()
        .find_map(|op| match op {
            Op::DefineFn(n, f) if n == name => Some(&**f),
            _ => None,
        })
        .unwrap()
}
fn transformed(source: &str, name: &str, optimize: bool) -> (usize, usize) {
    let mut heap = Heap::default();
    let mut ops = crate::frontend::compile(source, &mut heap).unwrap().ops;
    let before = function(&ops, name).locals.len();
    prepare(&mut ops, optimize);
    crate::op::check(&ops).unwrap();
    (before, function(&ops, name).locals.len())
}
fn source(n: usize, body: &str) -> String {
    let points = vec!["Point(1.0, 2.0)"; n].join(", ");
    format!(
        r#"@copy
class Point:
    x: f64
    y: f64

def kernel(values: &mut list[Point], steps: i64) -> ():
    count = len(values)
    for _ in range(steps):
        for i in range(count):
            {body}

def main() -> Result[(), AllocError]:
    mut points = [{points}]?
    kernel(&mut points, 3)
    Ok(())
"#
    )
}

#[test]
fn original_nbody_gains_scalar_state_without_source_changes() {
    let source = include_str!("../../../benchmarks/game/n-body/plenty/nbody.plenty");
    let (before, after) = transformed(source, "advance", true);
    assert_eq!(after - before, 75);
    let (before, after) = transformed(source, "advance", false);
    assert_eq!(before, after);
}

#[test]
fn different_record_names_and_sizes_are_supported() {
    for n in [2, 3, 6] {
        let text = source(
            n,
            "values[i].x = values[i].x + values[i].y\n            values[i].y = values[i].y * 0.5",
        );
        let (before, after) = transformed(&text, "kernel", true);
        assert_eq!(after - before, 2 * n, "size {n}");
    }
}

#[test]
fn unknown_calls_and_checked_integer_work_decline_promotion() {
    let text = source(
        3,
        "values[i].x = abs_value(values[i].x)\n            values[i].y = values[i].x",
    );
    let text = format!("def abs_value(x: f64) -> f64:\n    x * x\n\n{text}");
    let (before, after) = transformed(&text, "kernel", true);
    assert_eq!(before, after);
    let text = source(
        3,
        "values[i].x = values[i].x + values[i].y\n            values[i].y = values[i].y * 0.5",
    )
    .replace(
        "steps: i64) -> ():",
        "steps: i64) -> Result[(), OverflowError[i64]]:",
    )
    .replace(
        "    count = len(values)",
        "    mut total = steps\n    count = len(values)",
    )
    .replace("        for i", "        total += 1\n        for i")
    .replace("\ndef main()", "    Ok(())\n\ndef main()")
    .replace(
        "Result[(), AllocError]",
        "Result[(), AllocError | OverflowError[i64]]",
    )
    .replace("kernel(&mut points, 3)", "kernel(&mut points, 3)?");
    let (before, after) = transformed(&text, "kernel", true);
    assert_eq!(before, after);
}

#[test]
fn expansion_budget_rejects_large_and_ambiguous_shapes() {
    let text = source(
        9,
        "values[i].x = values[i].x + values[i].y\n            values[i].y = values[i].y * 0.5",
    );
    let (before, after) = transformed(&text, "kernel", true);
    assert_eq!(before, after);
    let text=source(3,"values[i].x = values[i].x + values[i].y\n            values[i].y = values[i].y * 0.5")
        .replace("    kernel(&mut points, 3)","    mut other = [Point(1.0, 2.0), Point(2.0, 3.0)]?\n    kernel(&mut other, 3)\n    kernel(&mut points, 3)");
    let (before, after) = transformed(&text, "kernel", true);
    assert_eq!(before, after);
}

#[test]
fn guarded_scratch_fixture_actually_promotes_cells() {
    let text = include_str!("../../../tests/fixtures/fixed-state.plenty");
    let (before, after) = transformed(text, "update", true);
    assert_eq!(after - before, 9);
}

#[test]
fn f32_state_uses_the_same_pass() {
    let text = source(
        3,
        "values[i].x = values[i].x + values[i].y\n            values[i].y = values[i].y * 0.5",
    )
    .replace("f64", "f32");
    let (before, after) = transformed(&text, "kernel", true);
    assert_eq!(after - before, 6);
}

#[test]
fn multiple_borrowed_roots_and_resize_are_not_promoted() {
    let text=source(3,"values[i].x = values[i].x + values[i].y\n            values[i].y = other[i].x")
        .replace("steps: i64) -> ()", "steps: i64, other: &list[Point]) -> ()")
        .replace("    kernel(&mut points, 3)","    other = [Point(1.0, 2.0), Point(1.0, 2.0), Point(1.0, 2.0)]?\n    kernel(&mut points, 3, &other)");
    let (before, after) = transformed(&text, "kernel", true);
    assert_eq!(before, after);
    let text = source(
        3,
        "values[i].x = values[i].x + values[i].y\n            values[i].y = values[i].y * 0.5",
    )
    .replace(
        "steps: i64) -> ():",
        "steps: i64) -> Result[(), AllocError]:",
    )
    .replace(
        "        for i",
        "        values.append(Point(0.0, 0.0))?\n        for i",
    )
    .replace("\ndef main()", "    Ok(())\n\ndef main()")
    .replace("kernel(&mut points, 3)", "kernel(&mut points, 3)?");
    let (before, after) = transformed(&text, "kernel", true);
    assert_eq!(before, after);
}

#[test]
fn moved_owned_scratch_keeps_the_destination_root() {
    let types = vec![Ty::List(Arc::new(Ty::F64)); 2];
    let hints = HashMap::default();
    let mut prefix = Eval::new(&types, &hints);
    prefix.run(&[Op::MoveLocal(0), Op::StoreLocal(1)]).unwrap();
    assert!(matches!(prefix.env[1], Val::Root(1)));
    let text = include_str!("../../../tests/fixtures/fixed-state.plenty")
        .replace("mut scratch =", "mut initial =")
        .replace("scratch.append", "initial.append")
        .replace(
            "    for _ in range(steps):",
            "    mut scratch = initial\n    for _ in range(steps):",
        );
    let (before, after) = transformed(&text, "update", true);
    assert_eq!(after - before, 9);
}
