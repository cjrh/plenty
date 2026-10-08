mod support;
use std::process::Command;

fn run(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{source}"
    );
}

fn reject(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let error = support::compile_source_to_executable(source, &directory.path().join("program"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected:?}, got {error}"
    );
}

const POINT: &str = r#"
class Point:
    x: i64
    y: i64
    def magnitude_squared(self) -> i64:
        self.x * self.x + self.y * self.y
    def shift(self: &mut Point, amount: i64) -> ():
        self.x = self.x + amount
        self.y = self.y + amount
"#;
const RESOURCE: &str = r#"
class Resource:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
"#;

#[test]
fn fields_constructors_methods_and_copy() {
    run(&format!("{POINT}\nmut p = Point(3, 4).unwrap()\nprint(p.magnitude_squared()).unwrap()\np.shift(1)\nmut q = copy(p).unwrap()\nq.x = 20\nprint(p).unwrap()\nprint(q).unwrap()\nprint(p == q).unwrap()\nprint(Point(2, 3).unwrap().magnitude_squared()).unwrap()"),
        "25\nPoint(x=4, y=5)\nPoint(x=20, y=5)\nFalse\n13\n");
}

#[test]
fn explicit_constructor_and_control_flow() {
    run(
        r#"
class Pair:
    x: i64
    y: i64
    def __init__(self, x: i64, positive: bool) -> ():
        if positive:
            self.x = x
        else:
            self.x = -x
        self.y = self.x + 1
print(Pair(3, False).unwrap()).unwrap()
"#,
        "Pair(x=-3, y=-2)\n",
    );
}

#[test]
fn nested_field_borrows_and_collection_mutation() {
    run(
        &format!(
            r#"{POINT}
class Drawing:
    origin: Point
    values: list[i64]
mut drawing = Drawing(Point(1, 2).unwrap(), [3].unwrap()).unwrap()
drawing.origin.shift(2)
r = &mut drawing.origin.x
*r = 8
drawing.values.append(len(drawing.values)).unwrap()
drawing.values[0] = 9
v = &drawing.origin
print(v.magnitude_squared()).unwrap()
print(drawing).unwrap()
"#
        ),
        "80\nDrawing(origin=Point(x=8, y=4), values=[9, 1])\n",
    );
}

#[test]
fn aliases_and_forward_field_types() {
    run(
        r#"
type P = Point
class Box:
    point: P
enum Value:
    Position(Box)
class Point:
    x: i64
print(Value.Position(Box(P(3).unwrap()).unwrap()).unwrap()).unwrap()
class Empty:
    pass
print(Empty().unwrap()).unwrap()
"#,
        "Value.Position(Box(point=Point(x=3)))\nEmpty()\n",
    );
}

#[test]
fn scope_move_replace_return_and_early_drop() {
    run(
        &format!(
            r#"{RESOURCE}
def make() -> Resource:
    Resource("returned").unwrap()
def consume(value: Resource) -> ():
    print("consume").unwrap()
def demo() -> ():
    a = Resource("first").unwrap()
    mut b = Resource("old").unwrap()
    b = Resource("replacement").unwrap()
    c = a
    drop(c)
    consume(make())
    print("end").unwrap()
demo()
"#
        ),
        "old\nfirst\nconsume\nreturned\nend\nreplacement\n",
    );
}

#[test]
fn parent_then_fields_and_synchronous_nested_drop() {
    run(
        &format!(
            r#"{RESOURCE}
class Parent:
    first: Resource
    second: Resource
    def __del__(self) -> ():
        print("parent").unwrap()
        drop(Resource("inside").unwrap())
        print("after").unwrap()
class Outer:
    first: Parent
    second: Resource
drop(Outer(Parent(Resource("a").unwrap(), Resource("b").unwrap()).unwrap(), Resource("c").unwrap()).unwrap())
print("finished").unwrap()
"#
        ),
        "parent\ninside\nafter\na\nb\nc\nfinished\n",
    );
}

#[test]
fn collection_destructor_order() {
    run(
        &format!(
            r#"{RESOURCE}
drop([Resource("a"), Resource("b")])
drop({{1: Resource("c"), 2: Resource("d")}})
"#
        ),
        "a\nb\nc\nd\n",
    );
}

#[test]
fn resource_cleanup_happens_after_tail_callee() {
    run(
        &format!(
            r#"{RESOURCE}
def callee() -> i64:
    print("callee").unwrap()
    42
def caller() -> i64:
    r = Resource("drop").unwrap()
    return callee()
print(caller()).unwrap()
"#
        ),
        "callee\ndrop\n42\n",
    );
}

#[test]
fn generator_capture_and_abandonment() {
    run(&format!(r#"{RESOURCE}
def values(a: Resource, b: Resource) -> Generator[i64]:
    c = Resource("local").unwrap()
    yield 1
    print("complete").unwrap()
drop(values(Resource("unstarted a").unwrap(), Resource("unstarted b").unwrap()))
mut it = values(Resource("a").unwrap(), Resource("b").unwrap())
print(next(it)).unwrap()
drop(it)
mut done = values(Resource("finished a").unwrap(), Resource("finished b").unwrap())
next(done)
next(done)
"#), "unstarted b\nunstarted a\nOption[i64].Some(1)\nlocal\nb\na\ncomplete\nlocal\nfinished b\nfinished a\n");
}

#[test]
fn custom_drop_can_read_and_mutate_fields() {
    run(
        &format!(
            r#"{RESOURCE}
class Wrapper:
    r: Resource
    values: list[i64]
    def __del__(self) -> ():
        self.r = Resource("new").unwrap()
        self.values.append(9).unwrap()
        print(self.values).unwrap()
drop(Wrapper(Resource("old").unwrap(), [1].unwrap()).unwrap())
"#
        ),
        "old\n[1, 9]\nnew\n",
    );
}

#[test]
fn initialization_errors() {
    for (body, diagnostic) in [
        ("pass", "fields not initialized"),
        ("self.x = self.y\n        self.y = 1", "not initialized"),
        (
            "if True:\n            self.x = 1\n        self.y = 2",
            "fields not initialized",
        ),
        (
            "print(self).unwrap()\n        self.x = 1\n        self.y = 2",
            "fields not initialized",
        ),
        ("return", "fields not initialized"),
        ("for self in [1]:\n            pass", "cannot rebind self"),
    ] {
        reject(
            &format!(
                "class C:\n    x: i64\n    y: i64\n    def __init__(self) -> ():\n        {body}"
            ),
            diagnostic,
        );
    }
}

#[test]
fn invalid_lifecycle_methods() {
    for (source, diagnostic) in [
        (
            "class C:\n    def __init__(self) -> i64:\n        1",
            "must return ()",
        ),
        (
            "class C:\n    def __del__(self, x: i64) -> ():\n        pass",
            "takes only self",
        ),
        (
            "class C:\n    def __del__(self: &C) -> ():\n        pass",
            "lifecycle methods require",
        ),
        (
            "class C:\n    def method() -> ():\n        pass",
            "self receiver",
        ),
    ] {
        reject(source, diagnostic);
    }
    reject(
        &format!("{RESOURCE}\nr = Resource('x').unwrap()\nr.__del__()"),
        "cannot be called directly",
    );
}

#[test]
fn owned_fields_and_copy_restrictions() {
    reject(
        &format!("{RESOURCE}\nr = Resource('x').unwrap()\ncopy(r).unwrap()"),
        "cannot be copied",
    );
    reject(
        &format!("{RESOURCE}\nr = [Resource('x').unwrap()].unwrap()\ncopy(r).unwrap()"),
        "cannot be copied",
    );
    reject(
        &format!("{RESOURCE}\nr = Option[Resource].Some(Resource('x').unwrap())\ncopy(r).unwrap()"),
        "cannot be copied",
    );
    reject(
        &format!("{RESOURCE}\nclass Box:\n    r: Resource\nb = Box(Resource('x').unwrap()).unwrap()\nx = b.r"),
        "cannot move out of a field",
    );
}

#[test]
fn field_borrow_conflicts_and_mutability() {
    reject(
        &format!("{POINT}\np = Point(1, 2).unwrap()\np.x = 3"),
        "mut binding",
    );
    reject(
        &format!("{POINT}\np = Point(1, 2).unwrap()\np.shift(3)"),
        "mut binding",
    );
    run(
        &format!("{POINT}\nmut p = Point(1, 2).unwrap()\nr = &p.x\np.y = 3\nprint(r).unwrap()"),
        "1\n",
    );
    reject(
        &format!("{POINT}\nmut p = Point(1, 2).unwrap()\nr = &p.x\ndrop(p)\nprint(r).unwrap()"),
        "moved",
    );
    reject(
        &format!("{POINT}\nmut p = Point(1, 2).unwrap()\nr = &mut p\n*r = Point(3, 4).unwrap()"),
        "cannot replace a whole class",
    );
    reject(
        &format!("{POINT}\np = Point(1, 2).unwrap()\nq = p\nprint(p).unwrap()"),
        "moved",
    );
}

#[test]
fn method_reference_arguments_reborrow_for_the_whole_call() {
    let source = r#"
class Counter:
    value: i64
    def add_to(self, destination: &mut i64, source: &i64) -> ():
        *destination = *destination + self.value + *source
mut n = 1
mut other = 2
c = Counter(3).unwrap()
destination = &mut n
source = &other
c.add_to(destination, source)
print(n).unwrap()
print(other).unwrap()
"#;
    run(source, "6\n2\n");
    reject(
        &format!("{POINT}\nmut p = Point(1, 2).unwrap()\nr = &p\np.shift(r.x)"),
        "conflicting borrow",
    );
}

#[test]
fn disjoint_fields_support_simultaneous_exclusive_loans() {
    run(
        &format!(
            r#"{POINT}
class Pair:
    left: Point
    right: Point
mut pair = Pair(Point(1, 2).unwrap(), Point(3, 4).unwrap()).unwrap()
x = &mut pair.left.x
y = &mut pair.left.y
right = &mut pair.right
*x = *x + 10
*y = *y + 20
right.shift(30)
print(*x).unwrap()
print(*y).unwrap()
print(pair.right).unwrap()
"#
        ),
        "11\n22\nPoint(x=33, y=34)\n",
    );
}

#[test]
fn projected_loans_still_reject_overlapping_places_and_parent_access() {
    for body in [
        "r = &mut p.x\ns = &p.x\nprint(r).unwrap()",
        "r = &p.x\np.shift(1)\nprint(r).unwrap()",
        "r = &mut p\ns = &mut r.x\nr.shift(1)\nprint(s).unwrap()",
    ] {
        reject(
            &format!("{POINT}\nmut p = Point(1, 2).unwrap()\n{body}"),
            "conflicting borrow",
        );
    }
    reject(
        &format!(
            r#"{POINT}
class Outer:
    point: Point
mut outer = Outer(Point(1, 2).unwrap()).unwrap()
x = &outer.point.x
outer.point = Point(3, 4).unwrap()
print(x).unwrap()
"#
        ),
        "conflicting borrow",
    );
}

#[test]
fn temporary_receivers_finish_at_expression_end() {
    run(
        &format!(
            r#"{RESOURCE}
class Wrapper:
    r: Resource
    def size(self) -> i64:
        len(self.r.name)
print(Wrapper(Resource("temporary").unwrap()).unwrap().size()).unwrap()
print("after").unwrap()
"#
        ),
        "9\ntemporary\nafter\n",
    );
}

#[test]
fn list_iteration_moves_and_drops_each_element() {
    run(
        &format!(
            r#"{RESOURCE}
for r in [Resource("a").unwrap(), Resource("b").unwrap()].unwrap():
    print("body").unwrap()
print("after").unwrap()
for r in [Resource("c").unwrap(), Resource("d").unwrap()].unwrap():
    break
print("broken").unwrap()
"#
        ),
        "body\na\nbody\nb\nafter\nc\nd\nbroken\n",
    );
}

#[test]
fn matched_payload_is_owned_by_its_binding() {
    run(
        &format!(
            r#"{RESOURCE}
match Option[Resource].Some(Resource("payload").unwrap()):
    case Option[Resource].Some(r):
        drop(r)
        print("after drop").unwrap()
    case Option[Resource].Nothing:
        pass
print("after match").unwrap()
"#
        ),
        "payload\nafter drop\nafter match\n",
    );
}

#[test]
fn generator_yields_transfer_class_ownership() {
    run(
        &format!(
            r#"{RESOURCE}
def resources() -> Generator[Resource]:
    yield Resource("a").unwrap()
    yield Resource("b").unwrap()
for r in resources():
    print("body").unwrap()
print("after").unwrap()
"#
        ),
        "body\na\nbody\nb\nafter\n",
    );
}

#[test]
fn class_copy_recursively_duplicates_owned_fields() {
    run(&format!(r#"{POINT}
class Shape:
    origin: Point
    vertices: list[Point]
mut original = Shape(Point(1, 2).unwrap(), [Point(3, 4).unwrap()].unwrap()).unwrap()
mut changed = copy(original).unwrap()
changed.origin.x = 9
changed.vertices.append(Point(5, 6).unwrap()).unwrap()
print(original).unwrap()
print(changed).unwrap()
"#), "Shape(origin=Point(x=1, y=2), vertices=[Point(x=3, y=4)])\nShape(origin=Point(x=9, y=2), vertices=[Point(x=3, y=4), Point(x=5, y=6)])\n");
}

#[test]
fn initialized_early_return_and_match_paths() {
    run(
        r#"
class Box:
    value: i64
    def __init__(self, option: Option[i64]) -> ():
        match option:
            case Option[i64].Some(x):
                self.value = x
                return
            case Option[i64].Nothing:
                self.value = 0
print(Box(Option[i64].Some(7)).unwrap()).unwrap()
print(Box(Option[i64].Nothing).unwrap()).unwrap()
"#,
        "Box(value=7)\nBox(value=0)\n",
    );
}

#[test]
fn stored_references_and_duplicate_members_are_rejected() {
    reject("class C:\n    field: &i64", "cannot be stored");
    reject("class C:\n    field: Generator[i64]", "cannot be stored");
    reject(
        "class C:\n    x: i64\n    def x(self) -> i64:\n        0",
        "duplicate",
    );
}

#[test]
fn observed_temporaries_survive_the_full_expression() {
    run(
        &format!(
            r#"{RESOURCE}
print([Resource("element").unwrap()].unwrap()[0].name).unwrap()
print(Resource("left").unwrap() == Resource("right").unwrap()).unwrap()
names = [Resource("item").unwrap().name for n in range(2)].unwrap()
print(names).unwrap()
"#
        ),
        "element\nelement\nFalse\nright\nleft\nitem\nitem\n[\"item\", \"item\"]\n",
    );
}

#[test]
fn methods_are_namespaced_and_generated_signatures_are_bounded() {
    run(
        "class C:\n    def len(self) -> i64:\n        5\nprint(C().unwrap().len()).unwrap()",
        "5\n",
    );
    let fields = (0..256)
        .map(|i| format!("    field{i}: i64\n"))
        .collect::<String>();
    reject(&format!("class Oversized:\n{fields}"), "at most 256");
}

#[test]
fn consumed_dictionary_values_transfer_cleanup_to_the_callee() {
    run(
        &format!(
            r#"{RESOURCE}
def consume(items: list[Resource]) -> ():
    for item in items:
        drop(item)
        print("after item").unwrap()
consume({{1: Resource("payload").unwrap()}}.unwrap().values().unwrap())
print("after call").unwrap()
"#
        ),
        "payload\nafter item\nafter call\n",
    );
}
