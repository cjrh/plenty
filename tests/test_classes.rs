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
        print(self.name)
"#;

#[test]
fn fields_constructors_methods_and_copy() {
    run(&format!("{POINT}\nmut p = Point(3, 4)\nprint(p.magnitude_squared())\np.shift(1)\nmut q = copy(p)\nq.x = 20\nprint(p)\nprint(q)\nprint(p == q)\nprint(Point(2, 3).magnitude_squared())"),
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
print(Pair(3, False))
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
mut drawing = Drawing(Point(1, 2), [3])
drawing.origin.shift(2)
r = &mut drawing.origin.x
*r = 8
drawing.values.append(len(drawing.values))
drawing.values[0] = 9
v = &drawing.origin
print(v.magnitude_squared())
print(drawing)
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
print(Value.Position(Box(P(3))))
class Empty:
    pass
print(Empty())
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
    Resource("returned")
def consume(value: Resource) -> ():
    print("consume")
def demo() -> ():
    a = Resource("first")
    mut b = Resource("old")
    b = Resource("replacement")
    c = a
    drop(c)
    consume(make())
    print("end")
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
        print("parent")
        drop(Resource("inside"))
        print("after")
class Outer:
    first: Parent
    second: Resource
drop(Outer(Parent(Resource("a"), Resource("b")), Resource("c")))
print("finished")
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
    print("callee")
    42
def caller() -> i64:
    r = Resource("drop")
    return callee()
print(caller())
"#
        ),
        "callee\ndrop\n42\n",
    );
}

#[test]
fn generator_capture_and_abandonment() {
    run(&format!(r#"{RESOURCE}
def values(a: Resource, b: Resource) -> Generator[i64]:
    c = Resource("local")
    yield 1
    print("complete")
drop(values(Resource("unstarted a"), Resource("unstarted b")))
mut it = values(Resource("a"), Resource("b"))
print(next(it))
drop(it)
mut done = values(Resource("finished a"), Resource("finished b"))
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
        self.r = Resource("new")
        self.values.append(9)
        print(self.values)
drop(Wrapper(Resource("old"), [1]))
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
            "print(self)\n        self.x = 1\n        self.y = 2",
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
        &format!("{RESOURCE}\nr = Resource('x')\nr.__del__()"),
        "cannot be called directly",
    );
}

#[test]
fn owned_fields_and_copy_restrictions() {
    reject(
        &format!("{RESOURCE}\nr = Resource('x')\ncopy(r)"),
        "cannot be copied",
    );
    reject(
        &format!("{RESOURCE}\nr = [Resource('x')]\ncopy(r)"),
        "cannot be copied",
    );
    reject(
        &format!("{RESOURCE}\nr = Option[Resource].Some(Resource('x'))\ncopy(r)"),
        "cannot be copied",
    );
    reject(
        &format!("{RESOURCE}\nclass Box:\n    r: Resource\nb = Box(Resource('x'))\nx = b.r"),
        "cannot move out of a field",
    );
}

#[test]
fn field_borrow_conflicts_and_mutability() {
    reject(&format!("{POINT}\np = Point(1, 2)\np.x = 3"), "mut binding");
    reject(
        &format!("{POINT}\np = Point(1, 2)\np.shift(3)"),
        "mut binding",
    );
    reject(
        &format!("{POINT}\nmut p = Point(1, 2)\nr = &p.x\np.y = 3\nprint(r)"),
        "conflicting borrow",
    );
    reject(
        &format!("{POINT}\nmut p = Point(1, 2)\nr = &p.x\ndrop(p)\nprint(r)"),
        "moved",
    );
    reject(
        &format!("{POINT}\nmut p = Point(1, 2)\nr = &mut p\n*r = Point(3, 4)"),
        "cannot replace a whole class",
    );
    reject(
        &format!("{POINT}\np = Point(1, 2)\nq = p\nprint(p)"),
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
c = Counter(3)
destination = &mut n
source = &other
c.add_to(destination, source)
print(n)
print(other)
"#;
    run(source, "6\n2\n");
    reject(
        &format!("{POINT}\nmut p = Point(1, 2)\nr = &p\np.shift(r.x)"),
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
print(Wrapper(Resource("temporary")).size())
print("after")
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
for r in [Resource("a"), Resource("b")]:
    print("body")
print("after")
for r in [Resource("c"), Resource("d")]:
    break
print("broken")
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
match Option[Resource].Some(Resource("payload")):
    case Option[Resource].Some(r):
        drop(r)
        print("after drop")
    case Option[Resource].Nothing:
        pass
print("after match")
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
    yield Resource("a")
    yield Resource("b")
for r in resources():
    print("body")
print("after")
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
mut original = Shape(Point(1, 2), [Point(3, 4)])
mut changed = copy(original)
changed.origin.x = 9
changed.vertices.append(Point(5, 6))
print(original)
print(changed)
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
print(Box(Option[i64].Some(7)))
print(Box(Option[i64].Nothing))
"#,
        "Box(value=7)\nBox(value=0)\n",
    );
}

#[test]
fn recursive_layout_and_stored_references_are_rejected() {
    reject("class C:\n    child: C", "recursive");
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
print([Resource("element")][0].name)
print(Resource("left") == Resource("right"))
names = [Resource("item").name for n in range(2)]
print(names)
"#
        ),
        "element\nelement\nFalse\nright\nleft\nitem\nitem\n[\"item\", \"item\"]\n",
    );
}

#[test]
fn methods_are_namespaced_and_generated_signatures_are_bounded() {
    run(
        "class C:\n    def len(self) -> i64:\n        5\nprint(C().len())",
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
        print("after item")
consume({{1: Resource("payload")}}.values())
print("after call")
"#
        ),
        "payload\nafter item\nafter call\n",
    );
}
