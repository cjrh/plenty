//! Intrinsic shared windows/chunks retain one source loan and need no slice check.
mod support;

fn run_profiles(source: &str, expected: &str) {
    for optimize in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("windows");
        plenty::compile_source_to_executable_with_options(
            source,
            &executable,
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .unwrap();
        let output = std::process::Command::new(executable).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}

#[test]
fn list_and_byte_windows_and_short_final_chunks() {
    run_profiles(
        r#"
def show(xs: &[i64]) -> Result[(), IoError]:
    for w in xs.windows(2):
        print(w)?
    for w in xs.chunks(2):
        print(w)?
    Ok(())
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3, 4, 5]?
    show(&xs[:])?
    for w in b"abcde".windows(3):
        print(w)?
    for w in b"abcde".chunks(2):
        print(w)?
    Ok(())
"#,
        "[1, 2]\n[2, 3]\n[3, 4]\n[4, 5]\n[1, 2]\n[3, 4]\n[5]\nb\"abc\"\nb\"bcd\"\nb\"cde\"\nb\"ab\"\nb\"cd\"\nb\"e\"\n",
    );
}

#[test]
fn unicode_width_counts_scalars_and_handles_inline_and_heap_text() {
    run_profiles(
        r#"
def main() -> Result[(), IoError]:
    for w in "aé🐈界z".windows(2):
        print(w, len(w))?
    for w in "aé🐈界z".chunks(2):
        print(w, len(w))?
    for w in "é猫".windows(1):
        print(w)?
    for w in "aé".chunks(9):
        print(w)?
    Ok(())
"#,
        "aé 2\né🐈 2\n🐈界 2\n界z 2\naé 2\n🐈界 2\nz 1\né\n猫\naé\n",
    );
}

#[test]
fn nonpositive_huge_short_and_empty_sources_yield_nothing() {
    run_profiles(
        r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2]?
    empty: list[i64] = []?
    for n in [-9223372036854775808, -1, 0, 3, 9223372036854775807]?:
        for w in xs.windows(n):
            print("unexpected", w)?
        for w in "é猫".windows(n):
            print("unexpected", w)?
    for n in [-9223372036854775808, -1, 0]?:
        for w in xs.chunks(n):
            print("unexpected", w)?
        for w in "é猫".chunks(n):
            print("unexpected", w)?
    for w in empty.windows(1):
        print("unexpected", w)?
    for w in "".chunks(1):
        print("unexpected", w)?
    for w in xs.chunks(9223372036854775807):
        print(w)?
    print("done")?
    Ok(())
"#,
        "[1, 2]\ndone\n",
    );
}

#[test]
fn source_is_borrowed_through_backedges_and_each_window_is_shared() {
    for body in ["xs.append(4)?", "taken = xs", "xs[0] = 7", "w[0] = 7"] {
        let source = format!(
            "def main() -> Result[(), AllocError]:\n    mut xs = [1, 2, 3]?\n    for w in xs.windows(2):\n        {body}\n    Ok(())\n"
        );
        for optimize in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            assert!(
                plenty::compile_source_to_executable_with_options(
                    &source,
                    &directory.path().join("invalid"),
                    &plenty::CompileOptions {
                        optimize,
                        ..Default::default()
                    }
                )
                .is_err(),
                "accepted {body}, optimize={optimize}"
            );
        }
    }
}

#[test]
fn retained_window_keeps_owner_borrowed_after_loop() {
    let source = r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3]?
    mut saved = &xs[:1]
    for w in xs.windows(2):
        saved = w
    xs.append(4)?
    print(saved)?
    Ok(())
"#;
    assert!(plenty::check_source(source).is_err());
}

#[test]
fn windows_can_forward_the_original_parameter_origin() {
    run_profiles(
        r#"
def first(xs: &[i64]) -> &[i64]:
    for w in xs.windows(2):
        return w
    xs
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    print(first(&xs[:]))?
    Ok(())
"#,
        "[1, 2]\n",
    );
}

#[test]
fn break_continue_and_nested_loops_keep_cursors_aligned() {
    run_profiles(
        r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3, 4]?
    for w in xs.windows(2):
        if w[0] == 1:
            continue
        for inner in w.windows(1):
            print(inner)?
        break
    for w in "aé🐈z".windows(2):
        if w == "aé":
            continue
        print(w)?
        break
    Ok(())
"#,
        "[2]\n[3]\né🐈\n",
    );
}

#[test]
fn earlier_shared_windows_can_overlap_and_outlive_next_advance() {
    run_profiles(
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3]?
    mut saved = &xs[:1]
    for w in xs.windows(2):
        print(saved, w)?
        saved = w
    print(saved)?
    xs.append(4)?
    print(xs)?
    Ok(())
"#,
        "[1] [1, 2]\n[1, 2] [2, 3]\n[2, 3]\n[1, 2, 3, 4]\n",
    );
}

#[test]
fn width_and_returned_view_source_evaluate_once() {
    run_profiles(
        r#"
def width(n: &mut i64) -> Result[i64, OverflowError[i64]]:
    *n += 1
    Ok(2)
def tail(xs: &[i64]) -> &[i64]:
    &xs[1:]
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    xs = [0, 1, 2, 3]?
    mut count = 0
    for w in tail(&xs[:]).windows(width(&mut count)?):
        print(w)?
    print(count)?
    Ok(())
"#,
        "[1, 2]\n[2, 3]\n1\n",
    );
}

#[test]
fn existing_exclusive_element_loan_conflicts_with_window_source() {
    let source = r#"
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3]?
    r = &mut xs[0]
    for w in xs.windows(2):
        print(*r, w)?
    Ok(())
"#;
    assert!(plenty::check_source(source).is_err());
}

#[test]
fn constructing_windows_needs_no_range_trap_or_runtime_helper() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("scan.plenty");
    std::fs::write(
        &path,
        r#"
def scan(xs: &[i64], width: i64) -> f64:
    mut total = 0.0
    for w in xs.windows(width):
        total += f64(len(w))
    total
def main() -> ():
    pass
"#,
    )
    .unwrap();
    for optimize in [false, true] {
        let dump = plenty::dump_file(
            &path,
            None,
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
            &plenty::DumpOptions {
                stages: vec![plenty::DumpStage::Asm],
                filter: Some("scan".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let assembly = &dump
            .files
            .iter()
            .find(|(name, _)| name == "asm.txt")
            .unwrap()
            .1;
        assert!(!assembly.contains("call"), "{assembly}");
        assert!(!assembly.contains("trap"), "{assembly}");
    }
}

#[test]
fn generic_and_wrapped_returned_views_keep_their_origin() {
    run_profiles(
        r#"
def tail[T](xs: &[T]) -> &[T]:
    &xs[1:]
def maybe(xs: &[i64]) -> Option[&[i64]]:
    Some(xs)
def fallible(xs: &[i64]) -> Result[&[i64], AllocError]:
    Ok(xs)
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    v = tail(&xs[:])
    for w in v.windows(2):
        print(w)?
    for w in maybe(&xs[:]).unwrap().windows(2):
        print(w)?
    for w in fallible(&xs[:])?.chunks(2):
        print(w)?
    for w in tail[i64](&[4, 5, 6]?).windows(2):
        print(w)?
    Ok(())
"#,
        "[2, 3]\n[1, 2]\n[2, 3]\n[1, 2]\n[3]\n[5, 6]\n",
    );
}

#[test]
fn shared_window_loans_cover_class_fields_and_nested_lists() {
    for body in [
        "owner.values.append(9)?",
        "owner = Boxed([9]?)",
        "rows[0].append(9)?",
    ] {
        let (setup, source) = if body.starts_with("rows") {
            ("mut rows = [[1, 2, 3]?]?", "rows[0]")
        } else {
            ("mut owner = Boxed([1, 2, 3]?)", "owner.values")
        };
        let source = format!(
            "class Boxed:\n    values: list[i64]\ndef main() -> Result[(), AllocError]:\n    {setup}\n    for w in {source}.windows(2):\n        {body}\n    Ok(())\n"
        );
        assert!(plenty::check_source(&source).is_err(), "{source}");
    }
}

#[test]
fn windows_in_closures_and_external_borrowing_generators() {
    run_profiles(
        r#"
def head_pairs(xs: &[i64]) -> Generator[i64]:
    for w in xs.windows(2):
        yield w[0]
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    show = def [&xs]() -> Result[(), IoError]:
        for w in xs.windows(2):
            print(w)?
        Ok(())
    show()?
    for x in head_pairs(&xs[:]):
        print(x)?
    Ok(())
"#,
        "[1, 2]\n[2, 3]\n1\n2\n",
    );
    let error = support::check_diagnostic(
        r#"
def bad() -> Generator[i64]:
    xs = [1, 2, 3].unwrap()
    for w in xs.windows(2):
        yield w[0]
def main() -> ():
    drop(bad())
"#,
    );
    assert!(
        error.contains("yield") || error.contains("suspend"),
        "{error}"
    );
}

#[test]
fn unsupported_stored_sources_are_rejected() {
    for expression in ["xs.windows(2)", "(len(w) for w in xs.windows(2))"] {
        let source = format!(
            "def main() -> Result[(), AllocError]:\n    xs = [1, 2, 3]?\n    result = {expression}\n    drop(result)\n    Ok(())\n"
        );
        assert!(
            plenty::check_source(&source).is_err(),
            "accepted {expression}"
        );
    }
}

#[test]
fn eager_window_comprehensions_use_shared_loop_plan() {
    run_profiles(
        r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3, 4, 5]?
    print([len(w) for w in xs.chunks(2)]?)?
    Ok(())
"#,
        "[2, 2, 1]\n",
    );
}

#[test]
fn temporary_owners_live_through_loop_and_cannot_escape() {
    run_profiles(
        r#"
def main() -> Result[(), AllocError | IoError]:
    for w in ([1, 2, 3]?).windows(2):
        print(w)?
    for w in (("a" + "é🙂")?).chunks(2):
        print(w)?
    Ok(())
"#,
        "[1, 2]\n[2, 3]\naé\n🙂\n",
    );
    let source = r#"
def main() -> Result[(), AllocError | IoError]:
    xs = [9]?
    mut kept = &xs[:]
    for w in ([1, 2, 3]?).windows(2):
        kept = w
    print(kept)?
    Ok(())
"#;
    let error = support::check_diagnostic(source);
    assert!(
        error.contains("borrow") || error.contains("outlive") || error.contains("scope"),
        "{error}"
    );
}

#[test]
fn window_loans_protect_width_evaluation_and_pipeline_rejection() {
    for expression in [
        "xs.windows(grow(&mut xs)?)",
        "iter(xs.windows(2)).map(def(w: &[i64]) -> i64: len(w)).sum()",
    ] {
        let source = format!(
            "def grow(xs: &mut list[i64]) -> Result[i64, AllocError]:\n    xs.append(4)?\n    Ok(2)\ndef main() -> Result[(), AllocError]:\n    mut xs = [1, 2, 3]?\n    for w in {expression}:\n        drop(w)\n    Ok(())\n"
        );
        assert!(plenty::check_source(&source).is_err(), "{source}");
    }
}
