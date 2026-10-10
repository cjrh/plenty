mod support;
use rstest::rstest;
use std::process::Command;

#[rstest]
#[case(
    "mut a = [1, 2].unwrap()\nmut b = copy(a).unwrap()\nb.append(3).unwrap()\nprint(a).unwrap()\nprint(b).unwrap()",
    "[1, 2]\n[1, 2, 3]\n"
)]
#[case(
    "mut a = [1].unwrap()\nmut b = a\nb.append(2).unwrap()\nprint(b).unwrap()",
    "[1, 2]\n"
)]
#[case(
    "def add(xs: &mut list[i64]) -> ():\n    xs.append(3).unwrap()\nmut a = [1, 2].unwrap()\nadd(&mut a)\nprint(a).unwrap()",
    "[1, 2, 3]\n"
)]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\nr.append(2).unwrap()\nprint(r).unwrap()\na.append(3).unwrap()\nprint(a).unwrap()",
    "[1, 2]\n[1, 2, 3]\n"
)]
#[case(
    "def size(xs: &list[i64]) -> i64:\n    len(xs)\na = [1, 2].unwrap()\nprint(size(&a)).unwrap()\nprint(a).unwrap()",
    "2\n[1, 2]\n"
)]
#[case(
    "mut x = 3\nr = &mut x\n*r = 7\nprint(*r).unwrap()\nprint(x).unwrap()",
    "7\n7\n"
)]
#[case("mut a = [[1].unwrap()].unwrap()\nmut b = copy(a).unwrap()\nmut child = copy(b[0]).unwrap()\nchild.append(2).unwrap()\nb[0] = child\nprint(a).unwrap()\nprint(b).unwrap()", "[[1]]\n[[1, 2]]\n")]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\ns = &r\nprint(s).unwrap()\nr.append(2).unwrap()\nprint(r).unwrap()",
    "[1]\n[1, 2]\n"
)]
#[case(
    "a = [1, 2].unwrap()\nfor x in &a:\n    print(x).unwrap()\nprint(a).unwrap()",
    "1\n2\n[1, 2]\n"
)]
#[case(
    "mut a = [1].unwrap()\ndrop(a)\na = [2].unwrap()\nprint(a).unwrap()",
    "[2]\n"
)]
#[case(
    "def write_byte(x: &mut i8) -> ():\n    *x = -12i8\nmut x = 1i8\nwrite_byte(&mut x)\nprint(x).unwrap()",
    "-12\n"
)]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\n*r = [2, 3].unwrap()\nprint(r).unwrap()\nprint(a).unwrap()",
    "[2, 3]\n[2, 3]\n"
)]
#[case("def add(xs: &mut list[i64]) -> ():\n    xs.append(len(xs)).unwrap()\ndef twice(xs: &mut list[i64]) -> ():\n    add(xs)\n    add(&mut xs)\nmut a = [7].unwrap()\ntwice(&mut a)\nprint(a).unwrap()", "[7, 1, 2]\n")]
#[case("def count(xs: &list[i64]) -> i64:\n    mut n = 0\n    for x in xs:\n        n = n + x\n    n\na = [1, 2, 3].unwrap()\nprint(count(&a)).unwrap()\nprint(a).unwrap()", "6\n[1, 2, 3]\n")]
#[case("def g() -> Generator[i64]:\n    yield 3\ndef advance(g: &mut Generator[i64]) -> Option[i64]:\n    next(g)\nmut it = g()\nprint(advance(&mut it)).unwrap()\nprint(next(it)).unwrap()", "Option[i64].Some(3)\nOption[i64].Nothing\n")]
#[case("def g() -> Generator[i64]:\n    mut a = [1].unwrap()\n    r = &mut a\n    r.append(2).unwrap()\n    yield len(a)\n    yield 3\nprint(list(g()).unwrap()).unwrap()", "[2, 3]\n")]
#[case(
    "mut a = [1].unwrap()\nr = &a\nif True:\n    print(r).unwrap()\nelse:\n    print(r).unwrap()\na.append(2).unwrap()\nprint(a).unwrap()",
    "[1]\n[1, 2]\n"
)]
#[case("mut a = [1].unwrap()\nmut n = 0\nwhile n < 3:\n    r = &a\n    print(len(r)).unwrap()\n    a.append(n).unwrap()\n    n = n + 1\nprint(a).unwrap()", "1\n2\n3\n[1, 0, 1, 2]\n")]
#[case(
    "mut a = [1, 2].unwrap()\na[len(a) - 1] = len(a) + 1\nprint(a).unwrap()",
    "[1, 3]\n"
)]
#[case("type R = Result[list[i64], str]\na = R.Ok([1].unwrap())\nb = copy(a).unwrap()\nmatch b:\n    case R.Ok(items):\n        mut x = items\n        x.append(2).unwrap()\n        print(x).unwrap()\n    case R.Err(_):\n        pass\nprint(str.repr(a).unwrap()).unwrap()", "[1, 2]\nResult[list[i64], str].Ok([1])\n")]
#[case(
    "mut a: list[i64] = [].unwrap()\nfor n in range(20_000):\n    a.append(n).unwrap()\nprint(len(a)).unwrap()\nprint(a[-1]).unwrap()",
    "20000\n19999\n"
)]
#[case("type View = &list[i64]\ndef length(v: View) -> i64:\n    len(v)\na = [1, 2].unwrap()\nprint(length(&a)).unwrap()", "2\n")]
#[case(
    "mut text = 'old'\nr = &mut text\n*r = ('new' + ' text').unwrap()\nprint(text).unwrap()",
    "new text\n"
)]
#[case("mut x = 1\nr = (&mut x)\n*r = *(&r)\nprint(*r).unwrap()", "1\n")]
fn native(#[case] source: &str, #[case] expected: &str) {
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected.as_bytes());
}

#[rstest]
#[case("a = [1].unwrap()\nb = a\nprint(a).unwrap()", "moved")]
#[case(
    "mut a = [1].unwrap()\nr = &a\na.append(2).unwrap()\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\nprint(a).unwrap()\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &a\ndrop(a)\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &a\nb = a\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case("a = [1].unwrap()\nr = &mut a", "mut binding")]
#[case("mut a = [1].unwrap()\nr = &a\ns = &mut r", "shared reference")]
#[case(
    "def f(a: &mut list[i64], b: &list[i64]) -> ():\n    pass\nmut a = [1].unwrap()\nf(&mut a, &a)",
    "conflicting borrow"
)]
#[case("def f() -> &i64:\n    x = 1\n    &x", "returned references")]
#[case("r = &[1].unwrap()[0]", "named collection")]
#[case(
    "mut a = [1].unwrap()\nr = &a\nwhile len(a) > 0:\n    a.append(2).unwrap()\n    print(r).unwrap()",
    "conflicting borrow"
)]
#[case("a = [1].unwrap()\ndrop(a)\nprint(a).unwrap()", "moved")]
#[case(
    "def g() -> Generator[i64]:\n    a = [1].unwrap()\n    r = &a\n    yield 1\n    print(r).unwrap()",
    "across yield"
)]
#[case(
    "mut a = [1].unwrap()\nr = &mut a\ns = &r\nr.append(2).unwrap()\nprint(s).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nfor x in &a:\n    a.append(x).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &a\nif True:\n    a = [2].unwrap()\nprint(r).unwrap()",
    "conflicting borrow"
)]
#[case(
    "mut a = [1].unwrap()\nr = &a\nwhile True:\n    print(r).unwrap()\n    a.append(2).unwrap()",
    "conflicting borrow"
)]
#[case(
    "def f(x: &list[i64], y: list[i64]) -> ():\n    pass\na = [1].unwrap()\nf(&a, a)",
    "conflicting borrow"
)]
#[case("def main() -> ():\n    x = 1\n    &x", "expected (), got &i64")]
#[case("x = 1\nprint(*x).unwrap()", "dereference requires")]
#[case("a = [1].unwrap()\nx = [&a].unwrap()", "cannot be stored")]
#[case(
    "def g() -> Generator[i64]:\n    yield 1\nmut it = g()\nfor x in &it:\n    pass",
    "borrowed generator iteration"
)]
#[case(
    "def g(x: &i64) -> Generator[i64]:\n    yield *x",
    "capture references"
)]
#[case("x = 1\ndrop(&x)", "owned value")]
#[case(
    "s = ('a' + 'b').unwrap()\nfor n in range(2):\n    print(s).unwrap()\n    drop(s)",
    "`s` is moved in a loop"
)]
#[case("n = 1\nwhile n > 0:\n    drop(n)", "`n` is moved in a loop")]
#[case("s = ('a' + 'b').unwrap()\ndrop((s))\nprint(s).unwrap()", "moved")]
fn diagnostics(#[case] source: &str, #[case] expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected}, got {error}"
    );
}

const POINT: &str = "class Point:\n    x: i64\n    y: i64\n    def shift(self: &mut Point, amount: i64) -> ():\n        self.x = self.x + amount\n\n";
const TOTAL: &str = "def total(v: list[i64]) -> i64:\n    len(v)\n\n";

/// Each program is complete, so every line and column below is the one a
/// learner sees. The whole message is compared: the conflicting use is the
/// primary position, and the notes carry the borrow or move that caused it.
#[rstest]
#[case::shared_then_modify(
    "",
    "    mut a = [1]?\n    r = &a\n    a.append(2)?\n    print(r)?\n",
    "4:5: conflicting borrow: cannot modify or exclusively borrow `a` while it is borrowed
  3:10: note: the shared borrow of `a` starts here
  5:11: note: the borrow is used again here"
)]
#[case::exclusive_then_read(
    "",
    "    mut a = [1]?\n    r = &mut a\n    print(a)?\n    print(r)?\n",
    "4:11: conflicting borrow: cannot read or borrow `a` while it is exclusively borrowed
  3:14: note: the exclusive borrow of `a` starts here
  5:11: note: the borrow is used again here"
)]
#[case::move_while_borrowed(
    "",
    "    mut a = [1]?\n    r = &a\n    b = a\n    print(r)?\n",
    "4:9: conflicting borrow: cannot move `a` while it is borrowed
  3:10: note: the shared borrow of `a` starts here
  5:11: note: the borrow is used again here"
)]
#[case::assign_while_borrowed(
    "",
    "    mut a = [1]?\n    r = &a\n    if True:\n        a = [2]?\n    print(r)?\n",
    "5:9: conflicting borrow: cannot assign to `a` while it is borrowed
  3:10: note: the shared borrow of `a` starts here
  6:11: note: the borrow is used again here"
)]
// The call that uses both arguments is the conflicting expression itself, so
// there is no separate later use to point at.
#[case::two_arguments(
    "def f(a: &mut list[i64], b: &list[i64]) -> ():\n    pass\n\n",
    "    mut a = [1]?\n    f(&mut a, &a)\n",
    "6:16: conflicting borrow: cannot read or borrow `a` while it is exclusively borrowed
  6:12: note: the exclusive borrow of `a` starts here"
)]
#[case::loop_over_borrow(
    "",
    "    mut a = [1]?\n    for x in &a:\n        a.append(x)?\n",
    "4:9: conflicting borrow: cannot modify or exclusively borrow `a` while it is borrowed
  3:15: note: the shared borrow of `a` starts here
  3:15: note: the borrow is used again here on the next loop iteration"
)]
#[case::use_on_next_iteration(
    "",
    "    mut a = [1]?\n    r = &a\n    mut n = 0\n    while n < 2:\n        print(r)?\n        a.append(n)?\n        n = n + 1\n",
    "7:9: conflicting borrow: cannot modify or exclusively borrow `a` while it is borrowed
  3:10: note: the shared borrow of `a` starts here
  6:15: note: the borrow is used again here on the next loop iteration"
)]
// Entering a loop for the first time is not a repeated iteration.
#[case::use_in_later_loop(
    "",
    "    mut a = [1]?\n    r = &a\n    a.append(2)?\n    mut n = 0\n    while n < 2:\n        print(r)?\n        n = n + 1\n",
    "4:5: conflicting borrow: cannot modify or exclusively borrow `a` while it is borrowed
  3:10: note: the shared borrow of `a` starts here
  7:15: note: the borrow is used again here"
)]
#[case::same_field(
    POINT,
    "    mut p = Point(1, 2)\n    r = &mut p.x\n    s = &p.x\n    print(r)?\n    print(s)?\n",
    "10:10: conflicting borrow: cannot read or borrow `p.x` while it is exclusively borrowed
  9:14: note: the exclusive borrow of `p.x` starts here
  11:11: note: the borrow is used again here"
)]
#[case::overlapping_fields(
    "class Point:\n    x: i64\n    y: i64\n\nclass Outer:\n    point: Point\n    tag: i64\n\n",
    "    mut outer = Outer(Point(1, 2), 3)\n    x = &outer.point.x\n    outer.point = Point(3, 4)\n    print(x)?\n",
    "12:5: conflicting borrow: cannot modify or exclusively borrow `outer.point` while `outer.point.x` is borrowed
  11:10: note: the shared borrow of `outer.point.x` starts here
  13:11: note: the borrow is used again here"
)]
#[case::tuple_element(
    "",
    "    mut pair = (1, [2]?)\n    r = &mut pair[1]\n    print(pair[1])?\n    r.append(3)?\n",
    "4:11: conflicting borrow: cannot read or borrow `pair` while `pair[1]` is exclusively borrowed
  3:14: note: the exclusive borrow of `pair[1]` starts here
  5:5: note: the borrow is used again here"
)]
// A reborrow is reported against the owner it reaches, with its own path.
#[case::field_reborrow(
    POINT,
    "    mut p = Point(1, 2)\n    r = &mut p\n    s = &mut r.x\n    r.shift(1)\n    print(s)?\n",
    "11:5: conflicting borrow: cannot modify or exclusively borrow `p` while `p.x` is exclusively borrowed
  10:14: note: the exclusive borrow of `p.x` starts here
  12:11: note: the borrow is used again here"
)]
#[case::shared_reborrow(
    "",
    "    mut a = [1]?\n    r = &mut a\n    s = &r\n    r.append(2)?\n    print(s)?\n",
    "5:5: conflicting borrow: cannot modify or exclusively borrow `a` while it is borrowed
  4:10: note: the shared borrow of `a` starts here
  6:11: note: the borrow is used again here"
)]
// The callee may return either field, so no field is named for the loan.
#[case::imprecise_returned_reference(
    "class Pair:\n    x: i64\n    y: i64\n\ndef select(p: &mut Pair, first: bool) -> &mut i64:\n    if first:\n        return &mut p.x\n    &mut p.y\n\n",
    "    mut pair = Pair(1, 2)\n    r = select(&mut pair, True)\n    pair.y = 3\n    print(r)?\n",
    "13:5: conflicting borrow: cannot modify or exclusively borrow `pair.y` while `pair` is exclusively borrowed
  12:21: note: the exclusive borrow of `pair` starts here
  14:11: note: the borrow is used again here"
)]
#[case::exclusive_capture(
    "",
    "    mut count = 0\n    mut change = def [&mut count]() -> ():\n        count = count + 1\n    print(count)?\n    change()\n",
    "5:11: conflicting borrow: cannot read `count` while it is exclusively borrowed
  3:18: note: the exclusive borrow of `count` starts here
  6:5: note: the borrow is used again here"
)]
#[case::shared_capture_moved_with_closure(
    "",
    "    mut values = [1]?\n    read = def [&values]() -> i64:\n        len(values)\n    moved = read\n    values.append(2)?\n    print(moved())?\n",
    "6:5: conflicting borrow: cannot modify or exclusively borrow `values` while it is borrowed
  3:12: note: the shared borrow of `values` starts here
  7:11: note: the borrow is used again here"
)]
#[case::context_manager(
    "class Counter:\n    count: i64\n    def __enter__(self: &mut Counter) -> ():\n        pass\n    def __exit__(self: &mut Counter) -> ():\n        pass\n\n",
    "    mut original = Counter(1)\n    with &mut original:\n        print(original.count)?\n",
    "11:15: conflicting borrow: cannot read or borrow `original.count` while `original` is exclusively borrowed
  10:15: note: the exclusive borrow of `original` starts here
  10:10: note: the borrow is held until the end of this `with` block"
)]
#[case::use_after_move(
    TOTAL,
    "    nums = [1, 2]?\n    print(total(nums))?\n    print(len(nums))?\n",
    "7:15: use of moved binding `nums`
  6:17: note: `nums` is moved here"
)]
#[case::possibly_moved(
    TOTAL,
    "    nums = [1, 2]?\n    if len(nums) > 1:\n        print(total(nums))?\n    print(len(nums))?\n",
    "8:15: use of possibly moved binding `nums`
  7:21: note: `nums` is moved here on some paths"
)]
// Moved on every path is definite; one of the move sites is kept.
#[case::moved_on_every_branch(
    TOTAL,
    "    nums = [1, 2]?\n    if len(nums) > 1:\n        print(total(nums))?\n    else:\n        drop(nums)\n    print(len(nums))?\n",
    "10:15: use of moved binding `nums`
  7:21: note: `nums` is moved here"
)]
// `inner` is out of scope at the use; the message names the declared binding.
#[case::moved_into_ended_scope(
    "",
    "    nums = [1, 2]?\n    if True:\n        inner = nums\n        print(inner)?\n    print(nums)?\n",
    "6:11: use of possibly moved binding `nums`
  4:17: note: `nums` is moved here on some paths"
)]
#[case::moved_in_loop(
    TOTAL,
    "    nums = [1, 2]?\n    for n in range(2):\n        print(total(nums))?\n",
    "7:21: `nums` is moved in a loop, so the next iteration would use a moved value; reinitialize it before continuing"
)]
fn located_diagnostics(#[case] items: &str, #[case] body: &str, #[case] expected: &str) {
    let source = format!("{items}def main() -> Result[(), Failure]:\n{body}    Ok(())\n");
    let error = support::check_source(&source).unwrap_err().to_string();
    assert_eq!(error, expected, "{source}");
}

#[test]
fn yield_diagnostic_names_the_borrow_and_its_later_use() {
    let error = support::check_source(
        "def g() -> Generator[i64]:\n    a = [1].unwrap()\n    r = &a\n    yield 1\n    print(r).unwrap()\n\ndef main() -> ():\n    pass\n",
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "4:5: a borrow of `a` cannot remain live across yield
  3:10: note: the shared borrow of `a` starts here
  5:11: note: the borrow is used again here"
    );
}

/// Positions and notes are diagnostics metadata only. A borrow still ends at
/// its last use, and distinct fields still do not conflict.
#[rstest]
#[case::owner_after_last_use(
    "",
    "    mut a = [1]?\n    r = &mut a\n    r.append(2)?\n    print(r)?\n    a.append(3)?\n    print(a)?\n"
)]
#[case::distinct_fields(
    POINT,
    "    mut p = Point(1, 2)\n    x = &mut p.x\n    y = &mut p.y\n    *x = 5\n    *y = 6\n    print(p.x + p.y)?\n"
)]
#[case::reborrow_ends_before_parent_use(
    POINT,
    "    mut p = Point(1, 2)\n    r = &mut p\n    s = &mut r.x\n    *s = 4\n    r.shift(1)\n    print(p.x)?\n"
)]
#[case::fresh_borrow_each_iteration(
    "",
    "    mut a = [1]?\n    for i in range(2):\n        r = &a\n        print(r)?\n        a.append(i)?\n"
)]
#[case::capture_after_last_call(
    "",
    "    mut count = 0\n    mut change = def [&mut count]() -> ():\n        count = count + 1\n    change()\n    print(count)?\n"
)]
#[case::move_on_one_branch_then_reassign(
    TOTAL,
    "    mut nums = [1, 2]?\n    if len(nums) > 1:\n        print(total(nums))?\n        nums = [3]?\n    print(len(nums))?\n"
)]
fn diagnostics_metadata_does_not_extend_borrows(#[case] items: &str, #[case] body: &str) {
    let source = format!("{items}def main() -> Result[(), Failure]:\n{body}    Ok(())\n");
    if let Err(error) = support::check_source(&source) {
        panic!("{source}\n{error}");
    }
}

#[test]
fn checked_file_diagnostics_name_the_source_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("moved.plenty");
    std::fs::write(
        &path,
        "def total(v: list[i64]) -> i64:\n    len(v)\n\ndef main() -> Result[(), Failure]:\n    nums = [1, 2]?\n    print(total(nums))?\n    print(len(nums))?\n    Ok(())\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--check")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let file = path.display();
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!(
            "error: {file}:7:15: use of moved binding `nums`\n  {file}:6:17: note: `nums` is moved here\n"
        )
    );
}
