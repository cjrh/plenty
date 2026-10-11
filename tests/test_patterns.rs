//! `match` patterns: type arguments come from the matched value, payload
//! positions nest, and `|` joins alternatives (issue #36).
mod support;

fn runs(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn diagnostic(source: &str) -> String {
    support::check_source(source).unwrap_err().to_string()
}

const DECLARATIONS: &str = "enum Choice[T]:
    Empty
    One(T)
    Two(T, T)
enum Shape:
    Dot
    Circle(i64)
    Rect(i64, i64)
enum Other:
    Dot
";

/// A function `f` whose body is one `match` over a parameter of type `ty`.
fn rejects(ty: &str, cases: &str) -> String {
    let mut source = format!("{DECLARATIONS}def f(value: {ty}) -> i64:\n    match value:\n");
    for line in cases.lines() {
        source.push_str("        ");
        source.push_str(line);
        source.push('\n');
    }
    source.push_str("def main() -> ():\n    pass\n");
    diagnostic(&source)
}

#[test]
fn the_matched_value_supplies_generic_arguments() {
    runs(
        r#"
enum Choice[T]:
    Empty
    One(T)
    Two(T, T)
type Words = Choice[str]
def describe(choice: Choice[str]) -> str:
    match choice:
        case Choice.Empty:
            "empty"
        case Choice.One(word):
            word
        case Choice.Two(_, second):
            second
def count[T](choice: &Choice[T]) -> i64:
    match choice:
        case Choice.Empty:
            0
        case Choice.One(_):
            1
        case Choice.Two(_, _):
            2
def aliased(choice: &Words) -> bool:
    match choice:
        case Words.Empty:
            True
        case _:
            False
print(describe(Choice[str].Empty)).unwrap()
print(describe(Choice[str].One("a longer word"))).unwrap()
print(describe(Choice[str].Two("first", "the second word"))).unwrap()
print(count(&Choice[i64].Two(1, 2))).unwrap()
print(count(&Choice[str].One("x"))).unwrap()
print(aliased(&Choice[str].Empty)).unwrap()
"#,
        "empty\na longer word\nthe second word\n2\n1\nTrue\n",
    );
}

#[test]
fn library_enums_match_without_type_arguments() {
    runs(
        r#"
sender, receiver = channel[list[i64]](1).unwrap()
sender.send([1].unwrap()).unwrap()
match sender.send_nowait([2, 3].unwrap()):
    case Ok(_):
        print("sent").unwrap()
    case Err(SendError.Full(message)):
        print(message).unwrap()
    case Err(SendError.Disconnected(_)):
        print("disconnected").unwrap()
print(receiver.recv().unwrap()).unwrap()
"#,
        "[2, 3]\n[1]\n",
    );
}

#[test]
fn patterns_reject_type_arguments_and_qualified_prelude_variants() {
    assert_eq!(
        rejects("Choice[i64]", "case Choice[i64].Empty:\n    0\ncase _:\n    1"),
        "13:20: a pattern takes no type arguments, because the matched value supplies them; write `Choice.Empty`"
    );
    assert_eq!(
        rejects("Option[i64]", "case Option[i64].Some(v):\n    v\ncase _:\n    1"),
        "13:20: a pattern takes no type arguments, because the matched value supplies them; write `Some`"
    );
    assert_eq!(
        rejects(
            "Option[Choice[i64]]",
            "case Some(Choice[i64].One(v)):\n    v\ncase _:\n    1"
        ),
        "13:25: a pattern takes no type arguments, because the matched value supplies them; write `Choice.One`"
    );
    assert_eq!(
        rejects(
            "Option[i64]",
            "case Option.Some(v):\n    v\ncase Nothing:\n    1"
        ),
        "13:14: `Some` takes no qualifier in a pattern; write `Some`"
    );
    assert_eq!(
        rejects(
            "Result[i64, i64]",
            "case Result.Ok(v):\n    v\ncase _:\n    1"
        ),
        "13:14: `Ok` takes no qualifier in a pattern; write `Ok`"
    );
}

#[test]
fn a_qualifier_must_name_the_matched_enum() {
    assert_eq!(
        rejects("Shape", "case Other.Dot:\n    0\ncase _:\n    1"),
        "13:14: expected Shape, got Other"
    );
    assert_eq!(
        rejects("Shape", "case Choice.Empty:\n    0\ncase _:\n    1"),
        "13:14: expected Shape, got Choice"
    );
    assert_eq!(
        rejects("Choice[i64]", "case Shape.Dot:\n    0\ncase _:\n    1"),
        "13:14: expected Choice[i64], got Shape"
    );
    assert_eq!(
        rejects(
            "Option[Shape]",
            "case Some(Other.Dot):\n    0\ncase _:\n    1"
        ),
        "13:19: expected Shape, got Other"
    );
    assert_eq!(
        rejects("Option[Shape]", "case Some(Ok(v)):\n    0\ncase _:\n    1"),
        "13:19: `Ok` matches only Result values; qualify user-defined variants"
    );
    assert_eq!(
        rejects("Shape", "case Shape.Square(_):\n    0\ncase _:\n    1"),
        "13:14: unknown variant `Square`"
    );
}

#[test]
fn nested_patterns_consume_owned_values() {
    runs(
        r#"
enum Shape:
    Dot
    Circle(i64)
    Rect(i64, i64)
enum Message:
    Quit
    Text(str, list[i64])
    Wrapped(Option[str])
def area(shape: Result[Option[Shape], str]) -> i64:
    match shape:
        case Ok(Some(Shape.Circle(radius))):
            radius * radius * 3
        case Ok(Some(Shape.Rect(width, height))):
            width * height
        case Ok(Some(Shape.Dot)):
            0
        case Ok(Nothing):
            -1
        case Err(_):
            -2
def text(message: Option[Message]) -> str:
    match message:
        case Some(Message.Text(body, _)):
            body
        case Some(Message.Wrapped(Some(body))):
            body
        case Some(Message.Wrapped(Nothing)):
            "nothing wrapped"
        case Some(Message.Quit):
            "quit"
        case Nothing:
            "no message"
def main() -> Result[(), Failure]:
    print(area(Ok(Some(Shape.Circle(2)))))?
    print(area(Ok(Some(Shape.Rect(3, 4)))))?
    print(area(Ok(Some(Shape.Dot))))?
    print(area(Ok(Nothing)))?
    print(area(Err("a long error message")))?
    print(text(Some(Message.Text("a long message body", [1, 2, 3]?))))?
    print(text(Some(Message.Wrapped(Some("a long wrapped body")))))?
    print(text(Some(Message.Wrapped(Nothing))))?
    print(text(Some(Message.Quit)))?
    print(text(Nothing))?
    Ok(())
"#,
        "12\n12\n0\n-1\n-2\na long message body\na long wrapped body\nnothing wrapped\nquit\nno message\n",
    );
}

// The runtime checks report any payload that is leaked or released twice, so
// these cover the positions a pattern skips as well as the ones it binds.
#[test]
fn unbound_parts_of_a_consumed_value_are_released_once() {
    runs(
        r#"
enum Pair:
    Both(list[str], Option[list[str]])
    Neither
def first(pair: Result[Pair, list[str]]) -> i64:
    match pair:
        case Ok(Pair.Both(_, Some(second))):
            len(second)
        case Ok(Pair.Both(first, Nothing)):
            len(first) + 10
        case Ok(Pair.Neither):
            -1
        case Err(_):
            -2
def ignore(pair: Result[Pair, list[str]]) -> i64:
    match pair:
        case Ok(Pair.Both(_, Some(_))):
            1
        case Ok(Pair.Both(_, _)):
            2
        case _:
            3
def main() -> Result[(), Failure]:
    for round in range(3):
        a = ["one long string", "two long strings"]?
        b = ["three long strings"]?
        print(first(Ok(Pair.Both(a, Some(b)))))?
        c = ["four long strings", "five long strings", "six"]?
        print(first(Ok(Pair.Both(c, Nothing))))?
        print(first(Ok(Pair.Neither)))?
        print(first(Err(["seven long strings"]?)))?
        d = ["eight long strings"]?
        e = ["nine long strings"]?
        print(ignore(Ok(Pair.Both(d, Some(e)))))?
        print(ignore(Ok(Pair.Both(["ten long strings"]?, Nothing))))?
        print(ignore(Err(["eleven long strings"]?)))?
    Ok(())
"#,
        &"1\n13\n-1\n-2\n1\n2\n3\n".repeat(3),
    );
}

#[test]
fn a_variant_pattern_matches_through_a_box() {
    runs(
        r#"
enum Tree:
    Leaf(i64)
    Node(Box[Tree], Box[Tree])
def leftmost(tree: &Tree) -> i64:
    match tree:
        case Tree.Leaf(value):
            *value
        case Tree.Node(Tree.Leaf(value), _):
            *value + 100
        case Tree.Node(Tree.Node(left, _), _):
            leftmost(left) + 1000
def total(tree: Tree) -> i64:
    match tree:
        case Tree.Leaf(value):
            value
        case Tree.Node(Tree.Leaf(left), Tree.Leaf(right)):
            left * right
        case Tree.Node(left, right):
            total(left) + total(right)
def grow(tree: &mut Tree) -> ():
    match tree:
        case Tree.Node(Tree.Leaf(value), _):
            *value = *value + 1
        case Tree.Node(left, _):
            grow(left)
        case Tree.Leaf(value):
            *value = *value + 50
def main() -> Result[(), Failure]:
    mut deep = Tree.Node(Box(Tree.Node(Box(Tree.Leaf(1))?, Box(Tree.Leaf(2))?))?, Box(Tree.Leaf(3))?)
    print(leftmost(&deep))?
    grow(&mut deep)
    print(leftmost(&deep))?
    print(total(deep))?
    mut flat = Tree.Node(Box(Tree.Leaf(5))?, Box(Tree.Leaf(6))?)
    grow(&mut flat)
    print(leftmost(&flat))?
    print(total(flat))?
    Ok(())
"#,
        "1001\n1002\n7\n106\n36\n",
    );
}

#[test]
fn nested_patterns_borrow_through_a_reference() {
    runs(
        r#"
def inner(value: &Result[Result[i64, i64], i64]) -> &i64:
    match value:
        case Ok(Ok(number)) | Ok(Err(number)) | Err(number):
            number
def writable(value: &mut Result[Result[i64, i64], i64]) -> &mut i64:
    match value:
        case Ok(Ok(number)):
            number
        case Ok(Err(number)) | Err(number):
            number
enum Pair:
    Both(Option[str], Option[list[i64]])
def lengths(pair: &Pair) -> i64:
    match pair:
        case Pair.Both(Some(text), Some(numbers)):
            len(text) + len(numbers)
        case Pair.Both(Some(text), Nothing):
            len(text)
        case Pair.Both(Nothing, _):
            0
def main() -> Result[(), Failure]:
    mut value: Result[Result[i64, i64], i64] = Ok(Err(7))
    print(*inner(&value))?
    number = writable(&mut value)
    *number = 9
    print(*inner(&value))?
    value = Err(3)
    other = writable(&mut value)
    *other = *other + 1
    print(*inner(&value))?
    mut pair = Pair.Both(Some("a long text value"), Some([1, 2, 3]?))
    print(lengths(&pair))?
    match &mut pair:
        case Pair.Both(Some(text), Some(numbers)):
            numbers.append(len(text))?
            *text = "short"
        case _:
            pass
    print(lengths(&pair))?
    print(lengths(&Pair.Both(Some("abc"), Nothing)))?
    print(lengths(&Pair.Both(Nothing, Nothing)))?
    Ok(())
"#,
        "7\n9\n4\n20\n9\n3\n0\n",
    );
}

#[test]
fn alternatives_share_a_body_and_its_bindings() {
    runs(
        r#"
enum Shape:
    Dot
    Circle(i64)
    Rect(i64, i64)
    Label(str, i64)
    Named(i64, str)
def kind(shape: &Shape) -> str:
    match shape:
        case Shape.Dot | Shape.Circle(_):
            "round"
        case Shape.Rect(_, _) | Shape.Label(_, _) | Shape.Named(_, _):
            "other"
def width(shape: &Shape) -> i64:
    match shape:
        case Shape.Circle(size) | Shape.Rect(size, _) | Shape.Label(_, size) | Shape.Named(size, _):
            *size
        case Shape.Dot:
            0
def name(shape: Option[Shape]) -> str:
    match shape:
        case Some(Shape.Label(text, _) | Shape.Named(_, text)):
            text
        case Some(Shape.Dot) | Nothing:
            "nothing"
        case Some(_):
            "unnamed"
def main() -> Result[(), Failure]:
    shapes = [Shape.Dot, Shape.Circle(2), Shape.Rect(3, 4), Shape.Label("a long label text", 5), Shape.Named(6, "a long shape name")]?
    for shape in &shapes:
        print(kind(shape))?
        print(width(shape))?
    for shape in shapes:
        print(name(Some(shape)))?
    print(name(Nothing))?
    Ok(())
"#,
        "round\n0\nround\n2\nother\n3\nother\n5\nother\n6\nnothing\nunnamed\nunnamed\na long label text\na long shape name\nnothing\n",
    );
}

#[test]
fn alternatives_inside_payloads_combine() {
    runs(
        r#"
enum Level:
    Low
    Mid
    High
enum Reading:
    Both(Level, Option[i64])
def rank(reading: Reading) -> i64:
    match reading:
        case Reading.Both(Level.Low | Level.Mid, Some(value)):
            value
        case Reading.Both(Level.High, Some(value)):
            value * 10
        case Reading.Both(Level.Low | Level.High, Nothing):
            -1
        case Reading.Both(Level.Mid, Nothing):
            -2
def main() -> Result[(), Failure]:
    print(rank(Reading.Both(Level.Low, Some(1))))?
    print(rank(Reading.Both(Level.Mid, Some(2))))?
    print(rank(Reading.Both(Level.High, Some(3))))?
    print(rank(Reading.Both(Level.Low, Nothing)))?
    print(rank(Reading.Both(Level.Mid, Nothing)))?
    print(rank(Reading.Both(Level.High, Nothing)))?
    Ok(())
"#,
        "1\n2\n30\n-1\n-2\n-1\n",
    );
}

#[test]
fn a_covered_combination_is_legal_while_every_alternative_still_matches() {
    runs(
        r#"
enum Level:
    Low(i64)
    High(i64)
enum Two:
    Both(Level, Level)
def rank(pair: Two) -> i64:
    match pair:
        case Two.Both(Level.Low(_), Level.Low(_)):
            0
        case Two.Both(Level.Low(a) | Level.High(a), Level.Low(b) | Level.High(b)):
            a * 10 + b
def view(pair: &Two) -> i64:
    match pair:
        case Two.Both(Level.Low(_), Level.Low(_)):
            0
        case Two.Both(Level.Low(a) | Level.High(a), Level.Low(b) | Level.High(b)):
            *a * 10 + *b
def main() -> Result[(), Failure]:
    pairs = [Two.Both(Level.Low(1), Level.Low(2)), Two.Both(Level.Low(1), Level.High(2)), Two.Both(Level.High(3), Level.Low(4)), Two.Both(Level.High(5), Level.High(6))]?
    for pair in &pairs:
        print(view(pair))?
    for pair in pairs:
        print(rank(pair))?
    Ok(())
"#,
        "0\n12\n34\n56\n0\n12\n34\n56\n",
    );
}

#[test]
fn alternatives_move_owned_payloads_out_once() {
    runs(
        r#"
enum Job[F]:
    Now(F)
    Later(i64, F)
    Named(str, list[str])
    Skip
def run[F: OnceCallable[[], i64]](job: Option[Job[F]]) -> i64:
    match job:
        case Some(Job.Now(work) | Job.Later(_, work)):
            work()
        case Some(Job.Named(_, names)):
            len(names)
        case Some(Job.Skip) | Nothing:
            -1
def now[F: OnceCallable[[], i64]](work: F) -> i64:
    run(Some(Job[F].Now(work)))
def later[F: OnceCallable[[], i64]](work: F) -> i64:
    run(Some(Job[F].Later(0, work)))
def unused[F: OnceCallable[[], i64]](work: F, names: list[str]) -> i64:
    nothing: Option[Job[F]] = Nothing
    run(Some(Job[F].Named("a long job name", names))) * 100 + run(Some(Job[F].Skip)) * 10 + run(nothing)
def main() -> Result[(), Failure]:
    base = 40
    names = ["a long first name", "b"]?
    first = def once [base]() -> i64:
        base + 2
    second = def once [base]() -> i64:
        base - 2
    third = def once [base]() -> i64:
        base
    print(now(first))?
    print(later(second))?
    print(unused(third, names))?
    Ok(())
"#,
        "42\n38\n189\n",
    );
}

#[test]
fn nested_matches_work_in_loops_generators_and_tail_calls() {
    runs(
        r#"
enum Step:
    Go(Option[i64])
    Stop
def descend(state: Option[Option[i64]], depth: i64) -> i64:
    match state:
        case Some(Some(value)):
            if value == 0:
                return depth
            descend(Some(Some(value - 1)), depth + 1)
        case Some(Nothing) | Nothing:
            -1
def steps(items: list[Step]) -> Generator[i64]:
    for item in items:
        match item:
            case Step.Go(Some(value)):
                yield value
                yield value * 2
            case Step.Go(Nothing) | Step.Stop:
                yield -1
def count(items: &list[Option[Step]]) -> i64:
    mut total = 0
    for item in items:
        match item:
            case Some(Step.Go(Some(value))):
                if *value > 10:
                    break
                total = total + *value
            case Some(Step.Go(Nothing)):
                continue
            case Some(Step.Stop) | Nothing:
                total = total + 100
    total
def main() -> Result[(), Failure]:
    print(descend(Some(Some(2000000)), 0))?
    for value in steps([Step.Go(Some(3)), Step.Stop, Step.Go(Nothing), Step.Go(Some(4))]?):
        print(value)?
    print(count(&[Some(Step.Go(Some(1))), Some(Step.Go(Nothing)), Nothing, Some(Step.Stop), Some(Step.Go(Some(11))), Nothing]?))?
    Ok(())
"#,
        "2000000\n3\n6\n-1\n-1\n4\n8\n201\n",
    );
}

#[test]
fn nested_exclusive_matches_change_payloads_in_place() {
    runs(
        r#"
class Counter:
    count: i64
    last: Option[Result[str, i64]]
    def record(self: &mut Counter) -> i64:
        match &mut self.last:
            case Some(Ok(text)):
                *text = "replaced text value"
                self.count = self.count + 1
            case Some(Err(code)):
                *code = *code + 1
            case Nothing:
                pass
        self.count
def fill(nested: &mut Option[Option[Option[i64]]]) -> ():
    match nested:
        case Some(Some(Some(value))):
            *value = *value + 1
        case Some(Some(inner)):
            *inner = Some(9)
        case Some(outer):
            *outer = Some(Nothing)
        case Nothing:
            pass
def main() -> Result[(), Failure]:
    mut counter = Counter(0, Some(Ok("original text value")))
    print(counter.record())?
    print(counter.last)?
    counter.last = Some(Err(4))
    print(counter.record())?
    print(counter.last)?
    mut nested: Option[Option[Option[i64]]] = Some(Nothing)
    for round in range(3):
        fill(&mut nested)
        print(nested)?
    Ok(())
"#,
        "1\nOption[Result[str, i64]].Some(Result[str, i64].Ok(\"replaced text value\"))\n1\nOption[Result[str, i64]].Some(Result[str, i64].Err(5))\nOption[Option[Option[i64]]].Some(Option[Option[i64]].Some(Option[i64].Nothing))\nOption[Option[Option[i64]]].Some(Option[Option[i64]].Some(Option[i64].Some(9)))\nOption[Option[Option[i64]]].Some(Option[Option[i64]].Some(Option[i64].Some(10)))\n",
    );
}

#[test]
fn nested_bindings_follow_the_borrow_rules() {
    let sibling = r#"
enum Pair:
    Both(Option[str], Option[list[i64]])
def main() -> Result[(), Failure]:
    mut pair = Pair.Both(Some("a long string value"), Some([1]?))
    match &mut pair:
        case Pair.Both(Some(text), numbers):
            *numbers = Nothing
            print(*text)?
        case _:
            pass
    print(pair)?
    Ok(())
"#;
    runs(
        sibling,
        "a long string value\nPair.Both(Option[str].Some(\"a long string value\"), Option[list[i64]].Nothing)\n",
    );
    let replaced =
        diagnostic(&sibling.replace("*numbers = Nothing", "pair = Pair.Both(Nothing, Nothing)"));
    assert!(
        replaced.contains("cannot assign to `pair` while it is exclusively borrowed"),
        "{replaced}"
    );
    let shared = diagnostic(
        "def main() -> ():\n    value: Option[Option[i64]] = Some(Some(1))\n    match &value:\n        case Some(Some(number)):\n            *number = 2\n        case _:\n            pass\n",
    );
    assert!(
        shared.contains("assignment requires an exclusive reference"),
        "{shared}"
    );
    let moved = diagnostic(
        "def main() -> Result[(), Failure]:\n    value: Option[Option[Box[i64]]] = Some(Some(Box(3)?))\n    match value:\n        case Some(Some(_)) | Nothing:\n            pass\n        case _:\n            pass\n    print(value)?\n    Ok(())\n",
    );
    assert!(moved.contains("use of moved binding `value`"), "{moved}");
    let capture = diagnostic(
        "def main() -> ():\n    text = \"outer\"\n    job = def [text](value: Option[Option[str]]) -> ():\n        match value:\n            case Some(Some(text)):\n                pass\n            case _:\n                pass\n",
    );
    assert!(
        capture.contains("cannot shadow closure capture `text` with a new binding"),
        "{capture}"
    );
}

#[test]
fn coverage_is_checked_through_nested_patterns() {
    assert_eq!(
        rejects(
            "Result[Option[Shape], str]",
            "case Ok(Some(Shape.Circle(r))):\n    r\ncase Err(_):\n    1"
        ),
        "12:11: non-exhaustive match; missing Ok(Some(Shape.Dot)), Ok(Some(Shape.Rect(_, _))), Ok(Nothing)"
    );
    assert_eq!(
        rejects(
            "Choice[Shape]",
            "case Choice.Two(Shape.Dot, Shape.Dot):\n    0\ncase Choice.Empty:\n    1"
        ),
        "12:11: non-exhaustive match; missing Choice.Two(Shape.Dot, Shape.Circle(_)), Choice.Two(Shape.Dot, Shape.Rect(_, _)), Choice.Two(Shape.Circle(_), _), Choice.Two(Shape.Rect(_, _), _), Choice.One(_)"
    );
    assert_eq!(
        rejects("Option[i64]", "case Some(v):\n    v"),
        "12:11: non-exhaustive match; missing Nothing"
    );
    assert_eq!(
        rejects(
            "Choice[Choice[Shape]]",
            "case Choice.Two(Choice.Two(Shape.Dot, Shape.Dot), Choice.Two(Shape.Dot, Shape.Dot)):\n    0"
        ),
        "12:11: non-exhaustive match; missing Choice.Two(Choice.Two(Shape.Dot, Shape.Dot), Choice.Two(Shape.Dot, Shape.Circle(_))), Choice.Two(Choice.Two(Shape.Dot, Shape.Dot), Choice.Two(Shape.Dot, Shape.Rect(_, _))), Choice.Two(Choice.Two(Shape.Dot, Shape.Dot), Choice.Two(Shape.Circle(_), _)), Choice.Two(Choice.Two(Shape.Dot, Shape.Dot), Choice.Two(Shape.Rect(_, _), _)), Choice.Two(Choice.Two(Shape.Dot, Shape.Dot), Choice.Empty), Choice.Two(Choice.Two(Shape.Dot, Shape.Dot), Choice.One(_)), Choice.Two(Choice.Two(Shape.Dot, Shape.Circle(_)), _), Choice.Two(Choice.Two(Shape.Dot, Shape.Rect(_, _)), _), and more"
    );
}

#[test]
fn unreachable_cases_and_alternatives_are_rejected() {
    assert_eq!(
        rejects(
            "Option[Shape]",
            "case Some(_):\n    0\ncase Some(Shape.Dot):\n    2\ncase Nothing:\n    1"
        ),
        "15:9: unreachable case"
    );
    assert_eq!(
        rejects(
            "Option[Shape]",
            "case Some(Shape.Dot) | Nothing:\n    0\ncase Some(Shape.Dot):\n    2\ncase _:\n    1"
        ),
        "15:9: unreachable case"
    );
    assert_eq!(
        rejects(
            "Shape",
            "case Shape.Dot:\n    0\ncase Shape.Dot:\n    2\ncase _:\n    1"
        ),
        "15:9: duplicate variant case"
    );
    assert_eq!(
        rejects(
            "Shape",
            "case Shape.Dot:\n    0\ncase Shape.Circle(_) | Shape.Dot:\n    2\ncase _:\n    1"
        ),
        "15:32: unreachable `|` alternative: earlier patterns already match every value it names"
    );
    assert_eq!(
        rejects(
            "Option[Shape]",
            "case Some(Shape.Dot | _ | Shape.Circle(_)):\n    0\ncase Nothing:\n    1"
        ),
        "13:35: unreachable `|` alternative: earlier patterns already match every value it names"
    );
    assert_eq!(
        rejects("Shape", "case _ | Shape.Dot:\n    0"),
        "13:18: unreachable `|` alternative: earlier patterns already match every value it names"
    );
}

#[test]
fn alternatives_bind_the_same_names_with_the_same_types() {
    assert_eq!(
        rejects(
            "Shape",
            "case Shape.Circle(r) | Shape.Rect(w, _):\n    0\ncase _:\n    1"
        ),
        "13:32: every `|` alternative must bind the same names, but this one does not bind `r`"
    );
    assert_eq!(
        rejects(
            "Shape",
            "case Shape.Circle(r) | Shape.Rect(r, h):\n    0\ncase _:\n    1"
        ),
        "13:32: every `|` alternative must bind the same names, but only this one binds `h`"
    );
    assert_eq!(
        rejects(
            "Shape",
            "case Shape.Circle(r) | Shape.Dot:\n    0\ncase _:\n    1"
        ),
        "13:32: every `|` alternative must bind the same names, but this one does not bind `r`"
    );
    assert_eq!(
        rejects("Result[i64, str]", "case Ok(v) | Err(v):\n    0"),
        "13:22: `v` must have one type in every `|` alternative, but it is i64 in the first and str here"
    );
    assert_eq!(
        rejects(
            "Choice[Option[i64]]",
            "case Choice.Two(Some(a), Some(a)):\n    0\ncase _:\n    1"
        ),
        "13:39: duplicate payload binding"
    );
    assert_eq!(
        rejects(
            "Choice[Option[i64]]",
            "case Choice.Two(Some(a) | Some(a), Some(a)):\n    0\ncase _:\n    1"
        ),
        "13:49: duplicate payload binding"
    );
}

#[test]
fn payload_positions_take_only_patterns() {
    assert_eq!(
        rejects("Option[Shape]", "case Some(Dot):\n    0\ncase _:\n    1"),
        "13:19: `Dot` is a variant of `Shape`, so it cannot name a payload binding; write `Shape.Dot` to match the variant"
    );
    assert_eq!(
        rejects(
            "Option[Shape]",
            "case Some(Circle(r)):\n    0\ncase _:\n    1"
        ),
        "13:19: qualify user-defined variants with their enum type"
    );
    assert_eq!(
        rejects("Shape", "case other:\n    0"),
        "13:14: qualify user-defined variants with their enum type"
    );
    assert_eq!(
        rejects("Option[i64]", "case Some(Shape.Dot):\n    0\ncase _:\n    1"),
        "13:19: a variant pattern requires an enum payload, but this payload has type i64; bind it with a name or skip it with `_`"
    );
    assert_eq!(
        rejects(
            "Option[Shape]",
            "case Some(Shape.Rect(a)):\n    0\ncase _:\n    1"
        ),
        "13:19: case `Rect` requires 2 payload bindings (no parentheses for nullary variants)"
    );
    assert_eq!(
        rejects("Option[i64]", "case Some(3):\n    0\ncase _:\n    1"),
        "13:19: literal patterns are not supported; `match` selects enum variants, so compare other values with an `if`/`elif` chain"
    );
    assert_eq!(
        rejects("Option[Option[i64]]", "case Some(Some(mut v)):\n    0\ncase _:\n    1"),
        "13:24: payload bindings cannot be declared `mut`; bind the payload and move it into a mutable local in the arm, as in `mut local = payload`, or match `&mut value` to change it in place"
    );
    assert_eq!(
        rejects(
            "Option[(i64, i64)]",
            "case Some((a, b)):\n    0\ncase _:\n    1"
        ),
        "13:19: tuple patterns are not supported; bind the tuple and unpack it in the arm"
    );
}

#[test]
fn pattern_size_limits_are_reported() {
    let deep = format!(
        "case {}_{}:\n    0\ncase _:\n    1",
        "Some(".repeat(65),
        ")".repeat(65)
    );
    assert_eq!(
        rejects("Option[i64]", &deep),
        "13:339: pattern nesting exceeds the implementation limit of 64"
    );
    let levels = "enum Level:\n    A\n    B\n    C\n    D\nenum Wide:\n    All(Level, Level, Level, Level, Level)\n";
    let any = "Level.A | Level.B | Level.C | Level.D";
    let wide = format!(
        "{levels}def f(value: Wide) -> i64:\n    match value:\n        case Wide.All({any}, {any}, {any}, {any}, {any}):\n            0\ndef main() -> ():\n    pass\n"
    );
    assert_eq!(
        diagnostic(&wide),
        "10:9: pattern expands to more than 256 `|` combinations"
    );
}
