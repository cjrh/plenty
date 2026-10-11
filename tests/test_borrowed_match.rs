//! Borrowed payloads refer to the owner's storage, including packed sum tags.
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

#[test]
fn shared_matches_preserve_recursive_owners() {
    runs(
        r#"
enum Chain:
    End
    Link(i64, Box[Chain])
def total(chain: &Chain) -> i64:
    match chain:
        case Chain.End:
            0
        case Chain.Link(value, rest):
            *value + total(rest)
chain = Chain.Link(3, Box(Chain.Link(7, Box(Chain.End).unwrap())).unwrap())
print(total(&chain)).unwrap()
print(total(&chain)).unwrap()
"#,
        "10\n10\n",
    );
}

#[test]
fn shared_and_mutable_payload_references_can_return_from_functions() {
    runs(
        r#"
def inner(value: &Result[Result[i64, i64], i64]) -> &i64:
    match value:
        case Ok(result):
            match result:
                case Ok(number):
                    number
                case Err(number):
                    number
        case Err(number):
            number
def writable(value: &mut Result[Result[i64, i64], i64]) -> &mut i64:
    match value:
        case Ok(result):
            match result:
                case Ok(number):
                    number
                case Err(number):
                    number
        case Err(number):
            number
mut value: Result[Result[i64, i64], i64] = Ok(Err(7))
print(*inner(&value)).unwrap()
number = writable(&mut value)
*number = 42
print(str.repr(value).unwrap()).unwrap()
"#,
        "7\nResult[Result[i64, i64], i64].Ok(Result[i64, i64].Err(42))\n",
    );
}

#[test]
fn mutable_siblings_and_nested_sum_replacement_preserve_variants() {
    runs(
        r#"
enum Pair:
    Both(i64, Option[Result[i64, i64]], list[i64])
mut pair = Pair.Both(1, Some(Ok(2)), [].unwrap())
match &mut pair:
    case Pair.Both(left, right, _):
        *left = 5
        match right:
            case Some(result):
                *result = Err(9)
            case Nothing:
                pass
        print(*left).unwrap()
print(pair).unwrap()
"#,
        "5\nPair.Both(5, Option[Result[i64, i64]].Some(Result[i64, i64].Err(9)), [])\n",
    );
}

#[test]
fn payload_loan_diagnostics_point_at_the_scrutinee_and_the_payload_use() {
    for (body, expected) in [
        (
            "    mut value = Some(1)\n    match &value:\n        case Some(number):\n            value = Nothing\n            print(*number)?\n        case Nothing:\n            pass\n",
            "5:13: conflicting borrow: cannot assign to `value` while it is borrowed\n  3:12: note: the shared borrow of `value` starts here\n  6:20: note: the borrow is used again here",
        ),
        (
            "    mut value = Some(1)\n    match &mut value:\n        case Some(number):\n            print(value)?\n            *number = 2\n        case Nothing:\n            pass\n",
            "5:19: conflicting borrow: cannot read `value` while it is exclusively borrowed\n  3:16: note: the exclusive borrow of `value` starts here\n  6:13: note: the borrow is used again here",
        ),
    ] {
        let source = format!("def main() -> Result[(), Failure]:\n{body}    Ok(())\n");
        let error = support::check_source(&source).unwrap_err().to_string();
        assert_eq!(error, expected, "{source}");
    }
    // The payload loan ends at its last use, inside the arm.
    let source = "def main() -> Result[(), Failure]:\n    mut value = Some(1)\n    match &mut value:\n        case Some(number):\n            *number = 2\n            print(value)?\n        case Nothing:\n            pass\n    value = Nothing\n    print(value)?\n    Ok(())\n";
    if let Err(error) = support::check_source(source) {
        panic!("{source}\n{error}");
    }
}

#[test]
fn payload_loans_reject_invalidation_and_escaping_storage() {
    for (source, expected) in [
        ("mut value = Some(1)\nmatch &value:\n    case Some(number):\n        value = Nothing\n        print(*number).unwrap()\n    case Nothing:\n        pass", "conflicting borrow"),
        ("mut value = Some(1)\nmatch &mut value:\n    case Some(number):\n        print(value).unwrap()\n        *number = 2\n    case Nothing:\n        pass", "conflicting borrow"),
        ("value = Some(1)\nmatch &value:\n    case Some(number):\n        *number = 2\n    case Nothing:\n        pass", "exclusive reference"),
        ("def bad(source: &i64) -> &i64:\n    value = Some(*source)\n    match &value:\n        case Some(number):\n            number\n        case Nothing:\n            source", "reference parameter"),
        ("match &Some(1):\n    case Some(number):\n        pass\n    case Nothing:\n        pass", "named binding"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}\n{error}");
    }
}

#[test]
fn mutable_matches_update_inline_enums_without_changing_copies() {
    runs(
        "enum Value:\n    Number(i64)\nmut value = Value.Number(1)\nother = value\nmatch &mut value:\n    case Value.Number(number):\n        *number = 2\nprint(other).unwrap()\nprint(value).unwrap()",
        "Value.Number(1)\nValue.Number(2)\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn recursive_class_traversal_and_nested_mutation_do_not_allocate() {
    runs(r#"
class Node:
    value: i64
    next: Option[Box[Node]]
def total(node: &Node) -> i64:
    match &node.next:
        case Some(next):
            node.value + total(next)
        case Nothing:
            node.value
def bump(node: &mut Node) -> ():
    node.value = node.value + 1
    match &mut node.next:
        case Some(next):
            bump(next)
        case Nothing:
            pass
mut root = Node(1, Some(Box(Node(2, Nothing)).unwrap()))
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
bump(&mut root)
answer = total(&root)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(answer).unwrap()
print(total(&root)).unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n5\n5\n");
}

#[test]
fn packed_reference_metadata_survives_forwarding_and_closure_capture() {
    runs(
        r#"
def forward(value: &mut Option[i64]) -> &mut Option[i64]:
    value
mut value: Option[Option[i64]] = Some(Some(4))
match &mut value:
    case Some(inner):
        ref = forward(inner)
        *ref = Nothing
    case Nothing:
        pass
print(value).unwrap()
mut number: Result[i64, str] = Ok(7)
match &mut number:
    case Ok(payload):
        mut change = def [&mut payload]() -> ():
            payload = payload + 2
        change()
        change()
    case Err(_):
        pass
print(str.repr(number).unwrap()).unwrap()
"#,
        "Option[Option[i64]].Some(Option[i64].Nothing)\nResult[i64, str].Ok(11)\n",
    );
}

#[test]
fn inline_range_and_marker_payloads_preserve_surrounding_tags() {
    runs(r#"
mut value: Option[Result[range[i64], AllocError]] = Some(Ok(range(3)))
match &mut value:
    case Some(result):
        match result:
            case Ok(numbers):
                *numbers = range(4, 9, 2)
            case Err(_):
                pass
    case Nothing:
        pass
print(value).unwrap()
match &mut value:
    case Some(result):
        *result = Err(AllocError.CapacityOverflow)
        match result:
            case Err(error):
                print(error).unwrap()
            case Ok(_):
                pass
    case Nothing:
        pass
print(value).unwrap()
"#, "Option[Result[range, AllocError]].Some(Result[range, AllocError].Ok(range(4, 9, 2)))\nAllocError.CapacityOverflow\nOption[Result[range, AllocError]].Some(Result[range, AllocError].Err(AllocError.CapacityOverflow))\n");
}

#[test]
fn replacement_and_early_exits_drop_owners_exactly_once() {
    runs(
        r#"
class Item:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def fail() -> Result[(), str]:
    Err("stop")
def visit(value: &Option[Item]) -> Result[(), str]:
    match value:
        case Some(item):
            print(item.label).unwrap()
            fail()?
        case Nothing:
            pass
    Ok(())
mut value = Some(Some(Item("old")))
match &mut value:
    case Some(inner):
        *inner = Some(Item("new"))
        print(str.repr(visit(inner)).unwrap()).unwrap()
    case Nothing:
        pass
drop(value)
"#,
        "old\nnew\nResult[(), str].Err(\"stop\")\nnew\n",
    );
}

#[test]
fn branch_local_last_uses_and_loop_exits_release_payload_loans() {
    runs(
        r#"
mut value = Some(1)
for n in range(3):
    match &mut value:
        case Some(number):
            *number = *number + 1
            if n == 0:
                continue
            break
        case Nothing:
            break
value = Some(10)
match &value:
    case Some(number):
        print(number).unwrap()
        value = Nothing
    case Nothing:
        pass
print(value).unwrap()
"#,
        "10\nOption[i64].Nothing\n",
    );
}

#[test]
fn collection_entries_and_returned_owned_enum_fields_keep_their_origins() {
    runs(
        r#"
enum Data:
    Values(i64, list[i64])
def first(value: &mut Data) -> &mut i64:
    match value:
        case Data.Values(number, _):
            number
mut items = [Data.Values(1, [2].unwrap())].unwrap()
number = first(&mut items[0])
*number = 7
match &mut items[0]:
    case Data.Values(_, values):
        values[0] = 9
print(items).unwrap()
"#,
        "[Data.Values(7, [9])]\n",
    );
}

#[test]
fn returned_and_projected_payloads_reject_mutation_until_last_use() {
    for source in [
        "def get(value: &Result[i64, i64]) -> &i64:\n    match value:\n        case Ok(n):\n            n\n        case Err(n):\n            n\nmut value: Result[i64, i64] = Ok(1)\nn = get(&value)\nvalue = Err(2)\nprint(n).unwrap()",
        "mut values = [Some(1)].unwrap()\nmatch &values[0]:\n    case Some(n):\n        values.clear()\n        print(n).unwrap()\n    case Nothing:\n        pass",
        "mut value = Some(Some(1))\nmatch &mut value:\n    case Some(inner):\n        match inner:\n            case Some(number):\n                *inner = Nothing\n                print(number).unwrap()\n            case Nothing:\n                pass\n    case Nothing:\n        pass",
        "mut value = Some(1)\nmatch &value:\n    case Some(n):\n        drop(value)\n        print(n).unwrap()\n    case Nothing:\n        pass",
        "def values() -> Generator[i64]:\n    value = Some(1)\n    match &value:\n        case Some(n):\n            yield *n\n            yield *n\n        case Nothing:\n            pass",
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains("borrow") || error.contains("moved"), "{source}\n{error}");
    }
}

#[test]
fn deep_inline_projection_preserves_ancestor_tags() {
    let depth = 64;
    let mut source = "type Sum0 = i64\n".to_owned();
    for i in 1..=depth {
        source.push_str(&format!("type Sum{i} = Option[Sum{}]\n", i - 1));
    }
    source.push_str("def change0(value: &mut Sum0) -> ():\n    *value = 99\n");
    for i in 1..=depth {
        source.push_str(&format!("def change{i}(value: &mut Sum{i}) -> ():\n    match value:\n        case Some(inner):\n            change{}(inner)\n        case Nothing:\n            pass\n", i - 1));
    }
    source.push_str("v0 = 1\n");
    for i in 1..depth {
        source.push_str(&format!("v{i}: Sum{i} = Some(v{})\n", i - 1));
    }
    source.push_str(&format!(
        "mut value: Sum{depth} = Some(v{})\nchange{depth}(&mut value)\n",
        depth - 1
    ));
    // Unwrapping all layers also verifies that writing the scalar preserved
    // every enclosing Some tag, including the tag at bit 127.
    // Two statements: one chain of 64 calls would exceed the nesting limit.
    source.push_str(&format!(
        "half = value{0}\nprint(half{0}).unwrap()\n",
        ".unwrap()".repeat(depth / 2)
    ));
    runs(&source, "99\n");
}

#[test]
fn borrowed_inline_generators_resume_replace_and_clean_up_original_frames() {
    runs(
        r#"
class Item:
    label: str
    def __del__(self) -> ():
        print(self.label).unwrap()
def numbers(item: Item) -> Generator[i64]:
    yield 1
    yield 2
mut wrapped = Some(Some(numbers(Item("first"))))
match &mut wrapped:
    case Some(inner):
        match inner:
            case Some(source):
                print(next(source)).unwrap()
                *source = numbers(Item("second"))
                print(next(source)).unwrap()
            case Nothing:
                pass
    case Nothing:
        pass
drop(wrapped)
"#,
        "Option[i64].Some(1)\nfirst\nOption[i64].Some(1)\nsecond\n",
    );
}

#[test]
fn unit_payloads_read_and_write_as_unit_without_losing_the_variant() {
    runs(
        r#"
def complete(value: &mut Result[(), i64]) -> ():
    match value:
        case Ok(done):
            *done = ()
            *done
        case Err(_):
            pass
mut value: Result[(), i64] = Ok(())
complete(&mut value)
print(str.repr(value).unwrap()).unwrap()
"#,
        "Result[(), i64].Ok(())\n",
    );
}

// Issue #41: before inline storage (B35), an enum whose payloads were all
// immutable shared heap storage, so `match &mut` rejected it. Every enum now
// owns its payload inline, whatever the payload types.
#[test]
fn mutable_matches_write_scalar_mixed_and_owned_payload_enums() {
    runs(
        r#"
class Counter:
    count: i64
enum Size:
    Width(i64)
    Height(i64)
enum Shape:
    Circle(i64)
    Named(str)
    Points(list[i64])
enum Holder:
    One(Counter)
    Many(list[Counter])
def grow(shape: &mut Shape) -> ():
    match shape:
        case Shape.Circle(radius):
            *radius = *radius * 2
        case Shape.Named(name):
            *name = "a replacement long enough to allocate"
        case Shape.Points(points):
            points.append(9).unwrap()
def raise(holder: &mut Holder) -> ():
    match holder:
        case Holder.One(counter):
            counter.count = counter.count + 41
        case Holder.Many(counters):
            counters[0].count = 6
def count(holder: &Holder) -> i64:
    match holder:
        case Holder.One(counter):
            counter.count
        case Holder.Many(counters):
            counters[0].count
mut size = Size.Height(3)
match &mut size:
    case Size.Width(width):
        *width = 0
    case Size.Height(height):
        *height = *height + 1
print(size).unwrap()
mut circle = Shape.Circle(3)
mut named = Shape.Named("an original long enough to allocate")
mut points = Shape.Points([1, 2].unwrap())
grow(&mut circle)
grow(&mut named)
grow(&mut points)
print(circle).unwrap()
print(named).unwrap()
print(points).unwrap()
mut one = Holder.One(Counter(1))
mut many = Holder.Many([Counter(5)].unwrap())
raise(&mut one)
raise(&mut many)
print(count(&one)).unwrap()
print(count(&many)).unwrap()
"#,
        "Size.Height(4)\nShape.Circle(6)\nShape.Named(\"a replacement long enough to allocate\")\nShape.Points([1, 2, 9])\n42\n6\n",
    );
}

#[test]
fn mutable_matches_write_generic_enum_payloads() {
    runs(
        r#"
enum Maybe[T]:
    Just(T)
    Empty
def bump(value: &mut Maybe[i64]) -> ():
    match value:
        case Maybe.Just(number):
            *number = *number + 1
        case Maybe.Empty:
            pass
mut number = Maybe[i64].Just(1)
number_copy = number
mut text = Maybe[str].Just("a")
text_copy = text
mut items = Maybe[list[i64]].Just([1].unwrap())
mut empty = Maybe[i64].Empty
bump(&mut number)
bump(&mut empty)
match &mut text:
    case Maybe.Just(value):
        *value = "b"
    case Maybe.Empty:
        pass
match &mut items:
    case Maybe.Just(value):
        value.append(2).unwrap()
    case Maybe.Empty:
        pass
print(number).unwrap()
print(number_copy).unwrap()
print(text).unwrap()
print(text_copy).unwrap()
print(items).unwrap()
print(empty).unwrap()
"#,
        "Maybe[i64].Just(2)\nMaybe[i64].Just(1)\nMaybe[str].Just(\"b\")\nMaybe[str].Just(\"a\")\nMaybe[list[i64]].Just([1, 2])\nMaybe[i64].Empty\n",
    );
}

// Each holder below received a copy of a copyable enum. Writing through
// `match &mut` on one holder must reach no other: this is what shared enum
// storage could not guarantee (issue #41).
#[test]
fn mutable_matches_of_copyable_enums_leave_other_holders_unchanged() {
    runs(
        r#"
enum Shape:
    Circle(i64)
    Named(str)
enum Slot:
    Filled(Shape)
    Vacant
class Drawing:
    shape: Shape
def store(shape: &mut Shape, radius: i64) -> ():
    match shape:
        case Shape.Circle(current):
            *current = radius
        case Shape.Named(name):
            *name = "a replacement long enough to allocate"
def widened(shape: Shape) -> Shape:
    mut local = shape
    store(&mut local, 6)
    local
original = Shape.Circle(1)
mut copied = original
mut shapes = [original, Shape.Named("an original long enough to allocate")].unwrap()
mut table: dict[str, Shape] = {"a": original}.unwrap()
mut drawing = Drawing(original)
mut slot = Slot.Filled(original)
slot_copy = slot
show = def [original]() -> Shape:
    original
store(&mut copied, 2)
store(&mut shapes[0], 3)
store(&mut table["a"], 4)
store(&mut drawing.shape, 5)
argument = widened(original)
match &mut slot:
    case Slot.Filled(shape):
        store(shape, 7)
    case Slot.Vacant:
        pass
mut name = shapes.get(1).unwrap()
store(&mut name, 0)
mut element = shapes.get(0).unwrap()
store(&mut element, 8)
mut part = shapes.slice(0, 1).unwrap()
store(&mut part[0], 10)
for shape in &mut shapes:
    store(shape, 9)
print(original).unwrap()
print(show()).unwrap()
print(copied).unwrap()
print(table).unwrap()
print(drawing.shape).unwrap()
print(argument).unwrap()
print(slot_copy).unwrap()
print(slot).unwrap()
print(name).unwrap()
print(element).unwrap()
print(part).unwrap()
print(shapes).unwrap()
"#,
        "Shape.Circle(1)\nShape.Circle(1)\nShape.Circle(2)\n{\"a\": Shape.Circle(4)}\nShape.Circle(5)\nShape.Circle(6)\nSlot.Filled(Shape.Circle(1))\nSlot.Filled(Shape.Circle(7))\nShape.Named(\"a replacement long enough to allocate\")\nShape.Circle(8)\n[Shape.Circle(10)]\n[Shape.Circle(9), Shape.Named(\"a replacement long enough to allocate\")]\n",
    );
}

// Copies of a scalar enum are independent inline values, so neither the copy
// nor the write behind `match &mut` may allocate to unshare storage.
#[cfg(feature = "runtime-checks")]
#[test]
fn mutable_matches_of_copied_scalar_enums_do_not_allocate() {
    runs(
        r#"
enum Shape:
    Circle(i64)
    Square(i64)
mut shape = Shape.Circle(3)
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
other = shape
match &mut shape:
    case Shape.Circle(radius):
        *radius = *radius + 1
    case Shape.Square(side):
        *side = 0
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(shape).unwrap()
print(other).unwrap()
"#,
        "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\nShape.Circle(4)\nShape.Circle(3)\n",
    );
}
