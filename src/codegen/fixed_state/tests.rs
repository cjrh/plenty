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
    let simd = Simd::default();
    let mut prefix = Eval::new(&types, &hints, &simd);
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

/// The original loop of the guard `prepare` leaves in `name`, with the ops
/// before it on the fallback path.
fn fallback(source: &str, name: &str) -> Vec<Op> {
    let mut heap = Heap::default();
    let mut ops = crate::frontend::compile(source, &mut heap).unwrap().ops;
    prepare(&mut ops, true);
    let mut body = &function(&ops, name).body[..];
    if let [Op::CheckedFunction { body: inner, .. }] = body {
        body = inner;
    }
    body.iter()
        .find_map(|op| match op {
            Op::If { otherwise, .. }
                if otherwise.iter().any(|op| matches!(op, Op::Loop { .. })) =>
            {
                Some(otherwise.to_vec())
            }
            _ => None,
        })
        .unwrap()
}
fn header_reads(ops: &[Op]) -> usize {
    ops.iter()
        .filter(|op| matches!(op, Op::Collection(C::ListLenRef(_) | C::ListRowsRef(_))))
        .count()
}

#[test]
fn fallback_reads_cached_headers_at_the_loops_that_use_them() {
    let source = include_str!("../../../benchmarks/game/n-body/plenty/nbody.plenty");
    let fallback = fallback(source, "advance");
    let [Op::Loop { body, .. }] = &fallback[..] else {
        panic!("header reads stayed before the fallback loop: {fallback:?}");
    };
    assert!(header_reads(body) > 0);
}

#[test]
fn fallback_keeps_header_reads_before_a_loop_it_did_not_fully_evaluate() {
    // With three points the second loop runs no iteration during evaluation,
    // so nothing shows that its body leaves the list's header alone.
    let body = "values[i].x = values[i].x + values[i].y";
    let text = source(3, body).replace(
        "        for i in range(count):",
        "        for j in range(3, count):\n            values[j].y = 0.0\n        for i in range(count):",
    );
    let (before, after) = transformed(&text, "kernel", true);
    assert_eq!(after - before, 6);
    let kept = fallback(&text, "kernel");
    assert!(matches!(kept.last(), Some(Op::Loop { .. })));
    assert!(header_reads(&kept) > 0, "{kept:?}");

    let sunk = fallback(&source(3, body), "kernel");
    assert!(matches!(&sunk[..], [Op::Loop { .. }]), "{sunk:?}");
}

fn vector_fixture(template: &str, vector: &str, values: &str) -> String {
    let (ty, lanes, suffix) = match vector {
        "f64x2" => ("f64", 2, ""),
        _ => ("f32", 4, "f32"),
    };
    let literals = |n: usize| vec![format!("1.5{suffix}"); n].join(", ");
    template
        .replace("VECTOR", vector)
        .replace("VALUES", values)
        .replace("HINT", &literals(2 * lanes))
        .replace("COUNT", &values.split(',').count().to_string())
        .replace("LANES", &lanes.to_string())
        .replace("LAST", &(2 * lanes - 1).to_string())
        .replace("ZERO", &format!("0.0{suffix}"))
        .replace("HALF", &format!("0.5{suffix}"))
        .replace("TYPE", ty)
}

#[test]
fn vector_view_accesses_read_and_write_promoted_cells() {
    // The fixed-state pass only recognizes vector calls when the backend can
    // lower them. Other hosts retain the scalar std.simd calls and the loop.
    let promoted = |cells| {
        if cfg!(target_arch = "x86_64") {
            cells
        } else {
            0
        }
    };
    let template = include_str!("../../../tests/fixtures/fixed-state-simd.plenty");
    // Five rows each of the parameter and both scratch lists.
    let text = vector_fixture(template, "f64x2", "1.0, 2.0, 3.0, 4.0, 5.0");
    let (before, after) = transformed(&text, "update", true);
    assert_eq!(after - before, promoted(15));
    // The vector functions are ordinary calls without optimization.
    let (before, after) = transformed(&text, "update", false);
    assert_eq!(before, after);
    let text = vector_fixture(template, "f32x4", "1.0f32, 2.0f32, 3.0f32, 4.0f32, 5.0f32");
    let (before, after) = transformed(&text, "update", true);
    assert_eq!(after - before, promoted(15));
    let template = include_str!("../../../tests/fixtures/fixed-state-simd-bounds.plenty");
    for (vector, cells) in [("f64x2", 2 + 2 + 2), ("f32x4", 4 + 4 + 4)] {
        let (before, after) = transformed(&vector_fixture(template, vector, ""), "update", true);
        assert_eq!(after - before, promoted(cells), "{vector}");
    }
    // Nothing reads the length of `roots`, so a vector past its appended
    // rows adds a cell and a length guard that fails for those rows.
    let template = include_str!("../../../tests/fixtures/fixed-state-simd.plenty");
    let text = vector_fixture(template, "f64x2", "1.0, 2.0, 3.0, 4.0, 5.0")
        .replace(".set(out, i)", ".set(out, 2)");
    let (before, after) = transformed(&text, "update", true);
    assert_eq!(after - before, promoted(16));
}

#[test]
fn vector_view_accesses_outside_the_guarded_rows_decline_promotion() {
    let template = include_str!("../../../tests/fixtures/fixed-state-simd.plenty");
    let text = vector_fixture(template, "f64x2", "1.0, 2.0, 3.0, 4.0, 5.0");
    for (from, to) in [
        // A vector past the appended length.
        ("s.get_f64x2(xs, i)", "s.get_f64x2(xs, 2)"),
        // A negative vector index.
        ("s.get_f64x2(xs, i)", "s.get_f64x2(xs, -1)"),
        // A view of part of the list has other rows and another length.
        ("xs: &[f64] = &scratch", "xs: &[f64] = &scratch[1:]"),
        (
            "out: &mut [f64] = &mut roots",
            "out: &mut [f64] = &mut roots[:4]",
        ),
        // No counted loop of appends suggests the length `len_f64x2` needs.
        ("    for _ in range(5):", "    for _ in range(extra):"),
    ] {
        assert!(text.contains(from), "{from}");
        let text = text.replace(from, to);
        let (before, after) = transformed(&text, "update", true);
        assert_eq!(before, after, "{to}");
    }
}

#[test]
fn clist_columns_are_promoted_like_list_rows() {
    let source = include_str!("../../../benchmarks/game/n-body/plenty/nbody_clist.plenty");
    let (before, after) = transformed(source, "advance", true);
    assert_eq!(after - before, 75);
    let (before, after) = transformed(source, "advance", false);
    assert_eq!(before, after);
    let text = include_str!("../../../tests/fixtures/fixed-state-clist.plenty");
    let (before, after) = transformed(text, "update", true);
    assert_eq!(after - before, 9);
}

#[test]
fn promoted_clist_loop_reads_and_writes_each_cell_once() {
    let text = include_str!("../../../tests/fixtures/fixed-state-clist.plenty");
    let mut heap = Heap::default();
    let mut ops = crate::frontend::compile(text, &mut heap).unwrap().ops;
    prepare(&mut ops, true);
    fn count(ops: &[Op], assumed: &mut usize, checked: &mut usize) {
        for op in ops {
            match op {
                Op::Assumed(inner) => {
                    assert!(matches!(
                        **inner,
                        Op::Collection(
                            C::Clist(ClistOp::Get(..) | ClistOp::Set(..)) | C::ElementRef(..)
                        )
                    ));
                    *assumed += 1;
                }
                Op::Collection(C::Clist(
                    ClistOp::Get(..)
                    | ClistOp::Set(..)
                    | ClistOp::GetView(..)
                    | ClistOp::SetView(..),
                )) => *checked += 1,
                Op::Loop { body, .. } => count(body, assumed, checked),
                _ => {}
            }
        }
    }
    let mut body = &function(&ops, "update").body[..];
    if let [Op::CheckedFunction { body: inner, .. }] = body {
        body = inner;
    }
    let fast = body
        .iter()
        .find_map(|op| match op {
            Op::If {
                then, otherwise, ..
            } if otherwise.iter().any(|op| matches!(op, Op::Loop { .. })) => Some(then),
            _ => None,
        })
        .unwrap();
    let (mut assumed, mut checked) = (0, 0);
    count(fast, &mut assumed, &mut checked);
    // Six clist cells and three scratch cells loaded, all nine written back;
    // the fast path keeps no checked clist access.
    assert_eq!((assumed, checked), (18, 0));
}

/// Float columns are cells whatever else the row holds; only an all-float
/// row supplies a size hint.
#[test]
fn float_columns_of_a_mixed_clist_row_are_cells_when_another_list_is_hinted() {
    // No hint remains: the scratch capacity is not a literal and the mixed
    // row's literal capacity is ignored.
    let text = include_str!("../../../tests/fixtures/fixed-state-clist.plenty");
    let tagged = text
        .replace("    y: f64\n", "    y: f64\n    tag: i64\n")
        .replace("Point(1.25, -0.0)", "Point(1.25, -0.0, 7)")
        .replace("Point(-2.5, 3.75)", "Point(-2.5, 3.75, 7)")
        .replace("Point(0.125, -7.0)", "Point(0.125, -7.0, 7)")
        .replace("Point(1.0, 2.0)", "Point(1.0, 2.0, 7)");
    assert!(tagged.contains("tag: i64") && tagged.contains("(1.0, 2.0, 7)"));
    let (before, after) = transformed(&tagged, "update", true);
    assert_eq!(before, after);
    // A hinted `list[f64]` selects the function; the `x` and `y` columns of
    // the tagged row are then promoted with the scratch cells.
    let hinted = tagged
        .replace("with_capacity(scratch_size)", "with_capacity(3)")
        .replace("count = len(values)", "count = 3");
    assert!(hinted.contains("list[f64].with_capacity(3)") && hinted.contains("count = 3"));
    let (before, after) = transformed(&hinted, "update", true);
    assert_eq!(after - before, 9);

    // Rows of `bool, f32, u8, f64` with a hinted `list[f64]` scratch.
    let mixed = include_str!("../../../tests/fixtures/fixed-state-clist-mixed.plenty");
    let (before, after) = transformed(mixed, "update", true);
    assert_eq!(after - before, 9);
    let mut heap = Heap::default();
    let mut ops = crate::frontend::compile(mixed, &mut heap).unwrap().ops;
    prepare(&mut ops, true);
    fn columns(ops: &[Op], found: &mut Vec<usize>) {
        for op in ops {
            match op {
                Op::Assumed(inner) => {
                    if let Op::Collection(C::Clist(ClistOp::Get(_, c) | ClistOp::Set(_, c))) =
                        &**inner
                    {
                        found.push(*c);
                    }
                }
                Op::CheckedFunction { body, .. } | Op::Loop { body, .. } => columns(body, found),
                Op::If {
                    then, otherwise, ..
                } => {
                    columns(then, found);
                    columns(otherwise, found);
                }
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    columns(&function(&ops, "update").body, &mut found);
    found.sort_unstable();
    // Three rows of the `y: f32` and `x: f64` columns, loaded and stored.
    assert_eq!(found, [1, 1, 1, 1, 1, 1, 3, 3, 3, 3, 3, 3]);

    let anchor = "            values[i].y = values[i].y * 1.5f32\n";
    assert!(mixed.contains(anchor));
    for line in [
        // Without the scratch hint the literal `clist[P].with_capacity(5)`
        // selects nothing.
        None,
        // An access to a column that is not a float is not modelled.
        Some("            values[i].k = values[i].k\n"),
        Some("            values[i].flag = not values[i].flag\n"),
        Some("            if values[i].flag:\n                values[i].x = 0.0\n"),
    ] {
        let text = match line {
            None => mixed.replace("with_capacity(3)", "with_capacity(steps)"),
            Some(line) => mixed.replace(anchor, &format!("{anchor}{line}")),
        };
        assert_ne!(text, mixed);
        let (before, after) = transformed(&text, "update", true);
        assert_eq!(before, after, "{line:?}");
    }
}

#[test]
fn clist_resizes_and_runtime_indices_decline_promotion() {
    let text = include_str!("../../../tests/fixtures/fixed-state-clist-resize.plenty");
    let (before, after) = transformed(text, "update", true);
    assert_eq!(after - before, 6);
    let anchor = "            values[i].y -= values[i].x\n";
    assert!(text.contains(anchor));
    for line in RESIZES {
        let text = text.replace(anchor, &format!("{anchor}        {line}\n"));
        let (before, after) = transformed(&text, "update", true);
        assert_eq!(before, after, "{line}");
    }
    // A negative index is normalized at run time.
    let from = "values[i].x * 0.5 + values[i].y";
    assert!(text.contains(from));
    let text = text.replace(from, "values[i].x * 0.5 + values[-1].y");
    let (before, after) = transformed(&text, "update", true);
    assert_eq!(before, after);
}

/// Every clist operation that can change the row count or move the columns,
/// as a statement of the stepped loop. `tests/test_fixed_state.rs` runs the
/// same list.
const RESIZES: [&str; 6] = [
    "values.append(P(0.0, 0.0))?",
    "values.reserve(1)?",
    "drop(values.remove(0))",
    "values.clear()",
    "drop(values.copy_row(0))",
    "drop(values[0:1]?)",
];

/// Clist roots other than one borrowed parameter.
#[test]
fn owned_and_two_width_clists_are_promoted() {
    // A borrowed clist and an owned one that may have no allocation: 3 rows
    // of 2 columns each.
    let pair = include_str!("../../../tests/fixtures/fixed-state-clist-pair.plenty");
    let (before, after) = transformed(pair, "update", true);
    assert_eq!(after - before, 12);
    // A clist received by value and moved into a mutable local.
    let owned = include_str!("../../../tests/fixtures/fixed-state-clist-owned.plenty");
    let (before, after) = transformed(owned, "update", true);
    assert_eq!(after - before, 6);
    // `f64` and `f32` columns of one row.
    let bits = include_str!("../../../tests/fixtures/fixed-state-clist-bits.plenty");
    let (before, after) = transformed(bits, "update", true);
    assert_eq!(after - before, 6);
}

/// `xs[i] -= ps[j].x * dt` holds the deferred read of a list cell on the
/// stack while the clist read stores its index in a temporary local. The
/// value does not mention that local, so the store leaves it unchanged.
#[test]
fn a_deferred_list_cell_is_kept_across_a_store_to_an_unrelated_local() {
    let text = include_str!("../../../tests/fixtures/fixed-state-deferred.plenty");
    let compound = ["xs[i] -= ps[j].x * dt", "xs[j] *= ps[i].v - xs[i]"];
    assert!(compound.iter().all(|line| text.contains(line)));
    // Written without compound assignments, no value is held across a
    // store: this form is promoted under the earlier rule as well, which
    // allowed no deferred value on the stack at a store.
    let plain = text
        .replace(compound[0], "xs[i] = xs[i] - ps[j].x * dt")
        .replace(compound[1], "xs[j] = xs[j] * (ps[i].v - xs[i])");
    let (before, after) = transformed(&plain, "update", true);
    assert_eq!(after - before, 9);
    // 3 `xs` cells and 3 rows of 2 columns.
    let (before, after) = transformed(text, "update", true);
    assert_eq!(after - before, 9);
}

/// List roots only, with compound assignments across two lists.
#[test]
fn a_borrowed_list_and_an_owned_record_list_are_promoted_together() {
    let text = include_str!("../../../tests/fixtures/fixed-state-list-pair.plenty");
    let (before, after) = transformed(text, "update", true);
    assert_eq!(after - before, 9);
}
