//! Python slice syntax `xs[a:b:c]` (issue #579): Python's clamping, copies of
//! the selected elements, and allocation failure as `Err`.
mod support;
use rstest::rstest;
use support::assert_visible_stdout as native;

/// Python's selection for `range(len)[start:stop:step]`, by brute force.
fn python(len: i64, start: Option<i64>, stop: Option<i64>, step: i64) -> Vec<i64> {
    let clamp = |bound: i64, low: i64, high: i64| {
        let bound = if bound < 0 { bound + len } else { bound };
        bound.clamp(low, high)
    };
    let (first, end) = if step > 0 {
        (
            start.map_or(0, |s| clamp(s, 0, len)),
            stop.map_or(len, |s| clamp(s, 0, len)),
        )
    } else {
        (
            start.map_or(len - 1, |s| clamp(s, -1, len - 1)),
            stop.map_or(-1, |s| clamp(s, -1, len - 1)),
        )
    };
    let mut selected = Vec::new();
    let mut at = first;
    while (step > 0 && at < end) || (step < 0 && at > end) {
        selected.push(at);
        at += step;
    }
    selected
}

fn bound(value: Option<i64>) -> String {
    value.map_or(String::new(), |n| n.to_string())
}

/// Every combination agrees with a Python model for lists, text, and bytes.
#[test]
fn slices_match_python_for_lists_text_and_bytes() {
    let bounds = [None, Some(-9), Some(-2), Some(0), Some(1), Some(4), Some(9)];
    let steps = [-3, -1, 1, 2];
    // One function per bound pair keeps each frame's temporaries small.
    let mut functions = String::from("import std.bytes\n");
    let mut calls = String::new();
    let mut expected = String::new();
    let digits = ['0', '1', '2', '3', 'é', '5'];
    for (n, (start, stop)) in bounds
        .iter()
        .flat_map(|&start| bounds.iter().map(move |&stop| (start, stop)))
        .enumerate()
    {
        functions.push_str(&format!(
            "def case{n}(xs: &list[i64], text: &str, data: &std.bytes.Bytes) -> Result[(), AllocError | IoError | std.bytes.BytesError]:\n"
        ));
        for step in steps {
            let slice = format!("{}:{}:{step}", bound(start), bound(stop));
            functions.push_str(&format!(
                "    print(xs[{slice}]?)?\n    print(text[{slice}]?)?\n    print(data[{slice}]?.to_list()?)?\n"
            ));
            let selected = python(6, start, stop, step);
            let list = selected.iter().map(i64::to_string).collect::<Vec<_>>();
            let codes = selected
                .iter()
                .map(|n| (48 + n).to_string())
                .collect::<Vec<_>>();
            let text: String = selected.iter().map(|&n| digits[n as usize]).collect();
            expected.push_str(&format!(
                "[{}]\n{text}\n[{}]\n",
                list.join(", "),
                codes.join(", ")
            ));
        }
        functions.push_str("    Ok(())\n");
        calls.push_str(&format!("    case{n}(&xs, &text, &data)?\n"));
    }
    let body = format!(
        "{functions}def main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:\n    xs = [0, 1, 2, 3, 4, 5]?\n    text = \"0123é5\"\n    data = std.bytes.from_bytes(b\"012345\")?\n{calls}    Ok(())\n"
    );
    native(&body, &expected);
}

#[test]
fn omitted_bounds_steps_and_extreme_values() {
    native(
        r#"
def main() -> Result[(), AllocError | IoError | RangeError]:
    xs = [0, 1, 2, 3, 4, 5]?
    print(xs[:]?)?
    print(xs[::-1]?)?
    print(xs[-2:]?)?
    print(xs[:-4]?)?
    print(xs[-9223372036854775808:9223372036854775807]?)?
    print(xs[9223372036854775807::-9223372036854775808]?)?
    print(xs[len(xs) - 2:]?)?
    step = NonZero.try_from(2)?
    print(xs[1::step]?)?
    print(str.repr(NonZero.try_from(0)).unwrap())?
    Ok(())
"#,
        "[0, 1, 2, 3, 4, 5]\n[5, 4, 3, 2, 1, 0]\n[4, 5]\n[0, 1]\n[0, 1, 2, 3, 4, 5]\n[5]\n[4, 5]\n[1, 3, 5]\nResult[NonZero[i64], RangeError].Err(RangeError.OutOfRange)",
    );
}

#[test]
fn list_slices_copy_owned_elements_and_share_handles() {
    native(
        r#"
def main() -> Result[(), AllocError | IoError]:
    mut rows = [[1, 2]?, [3]?, [4, 5, 6]?]?
    tail = rows[1:]?
    rows[1].append(99)?
    drop(rows)
    print(tail)?
    names = [("A" + "da")?, ("B" + "ea")?]?
    reversed = names[::-1]?
    drop(names)
    print(reversed)?
    s = Shared(5)?
    handles = [s.share(), s.share()]?
    copies = handles[1:]?
    print(len(copies))?
    print(*copies[0])?
    Ok(())
"#,
        "[[3], [4, 5, 6]]\n[\"Bea\", \"Ada\"]\n1\n5",
    );
}

#[test]
fn receivers_are_borrowed_and_operands_run_once_left_to_right() {
    native(
        r#"
class Data:
    items: list[i64]
def middle(xs: &list[i64]) -> Result[list[i64], AllocError]:
    return xs[1:3]
def data() -> list[i64]:
    print("receiver").unwrap()
    [1, 2, 3, 4].unwrap()
def index(n: i64) -> i64:
    print(n).unwrap()
    n
def main() -> Result[(), AllocError | IoError]:
    record = Data([10, 20, 30, 40]?)
    print(middle(&record.items)?)?
    print(record.items[::2]?)?
    print(data()[index(1):index(3)]?)?
    print(record.items)?
    Ok(())
"#,
        "[20, 30]\n[10, 30]\nreceiver\n1\n3\n[2, 3]\n[10, 20, 30, 40]",
    );
}

#[test]
fn text_slices_use_scalar_positions() {
    native(
        r#"
def main() -> Result[(), AllocError | IoError]:
    text = "Aé🙂Z"
    print(text[::-1]?)?
    print(text[1:3]?)?
    print(text[-1:]?)?
    print("hello world"[6:]?)?
    print("abcdef"[::2]?)?
    print(str.repr(text[3:1]).unwrap())?
    Ok(())
"#,
        "Z🙂éA\né🙂\nZ\nworld\nace\nResult[str, AllocError].Ok(\"\")",
    );
}

#[test]
fn byte_slices_return_owned_bytes_without_an_import() {
    native(
        r#"
def main() -> Result[(), AllocError | IoError]:
    head = b"GET /index"[0:3]?
    print(head.len())?
    print(head[0])?
    path = b"GET /index"[4:]?
    back = path[::-1]?
    print(back.len())?
    print(back[0])?
    print(back[5])?
    Ok(())
"#,
        "3\n71\n6\n120\n47",
    );
}

#[test]
fn tuple_slices_select_components_at_compile_time() {
    native(
        r#"
def main() -> Result[(), AllocError | IoError]:
    t = (1, "two", 3.0)
    print(t[1:]?)?
    print(t[::-1]?)?
    print(t[:1]?)?
    t[5:]?
    print((4, 5, 6)[-2:]?)?
    Ok(())
"#,
        "(\"two\", 3.0)\n(3.0, \"two\", 1)\n(1,)\n(5, 6)",
    );
}

#[test]
fn clist_slices_copy_strided_rows() {
    native(
        r#"
class Point:
    x: i64
    y: f64
def main() -> Result[(), AllocError | IoError | AppendError[Point]]:
    mut items = clist[Point]()
    for i in range(6):
        items.append(Point(x=i, y=0.5 * f64(i)))?
    part = items[1:5:2]?
    print(len(part))?
    print(part[0].x)?
    print(part[1].y)?
    back = items[::-1]?
    print(back[0].x)?
    print(items[:]? == items)?
    none = items[9:]?
    print(len(none))?
    Ok(())
"#,
        "2\n1\n1.5\n5\nTrue\n0",
    );
}

#[rstest]
#[case(
    "xs = [1, 2, 3].unwrap()\nys: list[i64] = xs[1:]",
    "slice returns Result[list[i64], AllocError]; use `?` to propagate allocation failure"
)]
#[case("xs = [1, 2, 3].unwrap()\nys = xs[::0]", "slice step cannot be zero")]
#[case(
    "xs = [1, 2, 3].unwrap()\nk = 2\nys = xs[::k]",
    "slice step must be `NonZero[i64]`, not `i64`"
)]
#[case("xs = [1, 2, 3].unwrap()\nys = xs[1.0:]", "expected i64, got f64")]
#[case(
    "mut xs = [1, 2, 3].unwrap()\nxs[1:2] = [5].unwrap()",
    "slice assignment is not supported; assign elements by index, or build a new list"
)]
#[case(
    "xs = [1, 2, 3].unwrap()\nys = xs[1:2, 0]",
    "a slice cannot have several dimensions"
)]
#[case("d = {1: 2}.unwrap()\ne = d[1:]", "dict does not support slicing")]
#[case(
    "t = (1, 2, 3)\nn = 1\nu = t[n:]",
    "tuple slice bounds must be integer literals; the result type depends on them"
)]
#[case(
    "t = (1, [1].unwrap())\nu = t[1:]",
    "tuple slices of affine components are not supported yet"
)]
#[case(
    "import std.bytes\ndef main() -> ():\n    xs = [std.bytes.from_bytes(b\"a\").unwrap()].unwrap()\n    ys = xs[1:]",
    "std.bytes.Bytes cannot be copied"
)]
fn invalid_slices_are_rejected(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

/// Phase guard: borrowed slice views are rejected until issue #581 (backlog
/// B39) lands; delete this test then.
#[rstest]
#[case("xs = [1, 2, 3].unwrap()\nys = &xs[1:3]")]
#[case("mut xs = [1, 2, 3].unwrap()\nys = &mut xs[1:3]")]
fn borrowed_slices_are_rejected_until_views_land(#[case] source: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains("borrowed slice views are not implemented yet; bind the copy first"),
        "{error}"
    );
}

#[cfg(feature = "runtime-checks")]
#[rstest]
#[case(
    "list[str]",
    "list[str]",
    "[(\"A\" + \"da\").unwrap(), (\"B\" + \"ea\").unwrap(), (\"C\" + \"am\").unwrap()].unwrap()",
    "values[::-1]?",
    "[\"Cam\", \"Bea\", \"Ada\"]"
)]
#[case(
    "list[list[i64]]",
    "list[list[i64]]",
    "[[1].unwrap(), [2].unwrap(), [3].unwrap()].unwrap()",
    "values[1:]?",
    "[[2], [3]]"
)]
#[case(
    "list[i64]",
    "list[i64]",
    "[1, 2, 3, 4].unwrap()",
    "values[1:3]?",
    "[2, 3]"
)]
#[case(
    "list[i64]",
    "list[i64]",
    "[1, 2, 3, 4].unwrap()",
    "values[3:1]?",
    "[]"
)]
#[case(
    "str",
    "str",
    "(\"Aé\" + \"🙂Zabcdefgh\").unwrap()",
    "values[::-1]?",
    "\"hgfedcbaZ🙂éA\""
)]
#[case(
    "std.bytes.Bytes",
    "i64",
    "std.bytes.from_bytes(b\"hello\").unwrap()",
    "values[1:]?.len()",
    "4"
)]
fn every_slice_allocation_failure_returns_err_and_keeps_the_source(
    #[case] ty: &str,
    #[case] result: &str,
    #[case] initial: &str,
    #[case] slice: &str,
    #[case] expected: &str,
) {
    let ok = format!("Result[{result}, AllocError].Ok({expected})");
    // Raise the failure budget until the slice succeeds; every earlier
    // allocation failure must surface as `Err` and leave the source intact.
    for budget in 0.. {
        assert!(budget < 16, "{slice} never succeeded");
        let source = format!(
            r#"
import std.bytes
def take(values: &{ty}) -> Result[{result}, AllocError]:
    Ok({slice})
def main() -> Result[(), AllocError | IoError]:
    values = {initial}
    print("__test_fail_allocations_after_{budget}__")?
    result = take(&values)
    print("__test_restore_allocations__")?
    print(str.repr(result)?)?
    print(str.repr(take(&values))?)?
    Ok(())
"#
        );
        let output = support::run(&source);
        assert!(
            output.status.success(),
            "{source}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let visible: Vec<&str> = stdout
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect();
        assert_eq!(visible[1], ok, "{source}");
        if visible[0] == ok {
            break;
        }
        assert_eq!(
            visible[0],
            format!("Result[{result}, AllocError].Err(AllocError.OutOfMemory)"),
            "{source}"
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn clist_slice_allocation_failure_returns_err_and_keeps_the_source() {
    for (budget, result) in [(0, "Err(AllocError.OutOfMemory)"), (8, "Ok(2)")] {
        native(
            &format!(
                r#"
class Point:
    x: i64
    y: f64
def take(items: &clist[Point]) -> Result[i64, AllocError]:
    part = items[::2]?
    Ok(len(part))
def main() -> Result[(), AllocError | IoError | AppendError[Point]]:
    mut items = clist[Point]()
    for i in range(4):
        items.append(Point(x=i, y=0.0))?
    print("__test_fail_allocations_after_{budget}__")?
    result = take(&items)
    print("__test_restore_allocations__")?
    print(str.repr(result)?)?
    print(len(items))?
    print(str.repr(take(&items))?)?
    Ok(())
"#
            ),
            &format!("Result[i64, AllocError].{result}\n4\nResult[i64, AllocError].Ok(2)"),
        );
    }
}

#[test]
fn generic_functions_and_anonymous_functions_slice() {
    native(
        r#"
def tail[T](xs: &list[T]) -> Result[list[T], AllocError]:
    return xs[1:]
def main() -> Result[(), AllocError | IoError]:
    print(tail(&[1, 2, 3]?)?)?
    print(tail(&["a", "b"]?)?)?
    reverse = def(s: &str) -> Result[str, AllocError]:
        s[::-1]
    print(reverse("abc")?)?
    Ok(())
"#,
        "[2, 3]\n[\"b\"]\ncba",
    );
}
