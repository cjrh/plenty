//! A class field of class type is stored flat: its record bytes sit at a fixed
//! offset inside the owner's record, with no slot word holding its address
//! (issue #547). Reads, writes, moves, borrows, destructors, copies,
//! collections and C handles behave as they did with the address layout.
mod support;
use support::assert_visible_stdout as native;

#[test]
fn nested_fields_read_write_move_and_borrow_three_levels_deep() {
    native(
        r#"
class Point:
    x: i64
    y: i64

class Segment:
    start: Point
    end: Point
    label: str

class Path:
    first: Segment
    second: Segment
    id: i64

def bump(p: &mut Point, by: i64) -> ():
    p.x += by
    p.y += by

def total(s: &Segment) -> i64:
    s.start.x + s.start.y + s.end.x + s.end.y

def path(n: i64) -> Result[Path, AllocError]:
    a = Segment(Point(n, n + 1), Point(n + 2, n + 3), ("a" + str.from(n)?)?)
    b = Segment(Point(-n, 0), Point(0, -n), "b")
    Ok(Path(a, b, n))

def through(p: Path) -> Path:
    p

def main() -> Result[(), AllocError | IoError]:
    mut p = through(path(1)?)
    print(p)?
    bump(&mut p.first.end, 10)
    p.second.start = Point(7, 8)
    p.second.start.y = 9
    print(p.first.end, p.second.start, total(&p.first), total(&p.second))?
    seg = p.first
    print(seg.label, seg.end.x)?
    end = seg.end
    print(end)?
    p.first = Segment(Point(0, 0), end, "fresh")
    print(p.first, p.id)?
    mut moved = p
    moved.second.label = "renamed"
    print(moved.second)?
    copied = copy(&moved.first.end)?
    print(copied == moved.first.end, copied)?
    print(str.repr(Some(moved.second))?)?
    Ok(())
"#,
        "Path(first=Segment(start=Point(x=1, y=2), end=Point(x=3, y=4), label=\"a1\"), second=Segment(start=Point(x=-1, y=0), end=Point(x=0, y=-1), label=\"b\"), id=1)\nPoint(x=13, y=14) Point(x=7, y=9) 30 15\na1 13\nPoint(x=13, y=14)\nSegment(start=Point(x=0, y=0), end=Point(x=13, y=14), label=\"fresh\") 1\nSegment(start=Point(x=7, y=9), end=Point(x=0, y=-1), label=\"renamed\")\nTrue Point(x=13, y=14)\nOption[Segment].Some(Segment(start=Point(x=7, y=9), end=Point(x=0, y=-1), label=\"renamed\"))\n",
    );
}

#[test]
fn destructors_run_once_for_flat_fields_replaced_moved_or_released() {
    // A replaced, moved-out or popped flat field is released exactly once;
    // an owner's `__del__` runs before its fields', in declaration order.
    native(
        r#"
class Guard:
    name: str
    def __del__(self) -> ():
        print("drop", self.name).unwrap()

class Pair:
    left: Guard
    right: Guard

class Outer:
    pair: Pair
    tail: Guard
    def __del__(self) -> ():
        print("drop outer", self.tail.name).unwrap()

def make(tag: str) -> Result[Outer, AllocError]:
    Ok(Outer(Pair(Guard((tag + ".l")?), Guard((tag + ".r")?)), Guard((tag + ".t")?)))

def take_left(p: Pair) -> Guard:
    p.left

def main() -> Result[(), AllocError | IoError]:
    mut o = make("a")?
    o.pair.left = Guard("a.l2")
    print("replaced")?
    old = replace(o.pair.right, Guard("a.r2"))
    print("swapped out", old.name)?
    drop(old)
    b = Pair(Guard("b.l"), Guard("b.r"))
    left = take_left(b)
    print("kept", left.name)?
    mut items: list[Outer] = [make("c")?, make("d")?]?
    first = items.pop()
    print("popped")?
    drop(first)
    print("end")?
    Ok(())
"#,
        "drop a.l\nreplaced\nswapped out a.r\ndrop a.r\ndrop b.r\nkept b.l\npopped\ndrop outer d.t\ndrop d.l\ndrop d.r\ndrop d.t\nend\ndrop outer c.t\ndrop c.l\ndrop c.r\ndrop c.t\ndrop b.l\ndrop outer a.t\ndrop a.l2\ndrop a.r2\ndrop a.t\n",
    );
}

#[test]
fn flat_fields_cross_collections_enums_generics_closures_and_threads() {
    // Key sorting, dict values, enum payloads, a generic holder, a captured
    // borrow, a channel and a thread each hold or name nested records.
    native(
        r#"
class Point:
    x: i64
    y: i64

class Span:
    lo: Point
    hi: Point

class Tagged:
    span: Span
    name: str

enum Shape:
    Empty
    One(Span)
    Two(Span, i64)

class Holder[T]:
    item: T
    count: i64

def area(s: &Span) -> i64:
    (s.hi.x - s.lo.x) * (s.hi.y - s.lo.y)

def spans(n: i64) -> Generator[Span]:
    for i in range(n):
        yield Span(Point(0, 0), Point(i, i + 1))

def worker(t: Tagged) -> Tagged:
    Tagged(Span(t.span.hi, t.span.lo), t.name)

def width(t: &Tagged) -> i64:
    t.span.hi.x - t.span.lo.x

def main() -> Result[(), AllocError | IoError | ThreadError | ChannelError | SendError[Tagged] | RecvError]:
    mut tags: list[Tagged] = []?
    for i in range(4):
        tags.append(Tagged(Span(Point(i, 0), Point(10 - i * 3, 5)), ("t" + str.from(i)?)?))?
    tags.sort_by_key(width)
    for t in &tags:
        print(t.name, width(t))?
    mut by_name: dict[str, Span] = {}?
    for s in spans(3):
        key = str.from(area(&s))?
        by_name.insert(key, s)?
    print(len(by_name), by_name["2"], by_name["6"] == Span(Point(0, 0), Point(2, 3)))?
    shapes = [Shape.One(Span(Point(1, 1), Point(3, 4))), Shape.Two(Span(Point(0, 0), Point(5, 5)), 2), Shape.Empty]?
    for shape in &shapes:
        match shape:
            case Shape.One(s):
                print(area(s))?
            case Shape.Two(s, k):
                print(area(s) * *k)?
            case Shape.Empty:
                print("empty")?
    h = Holder[Span](Span(Point(1, 2), Point(3, 4)), 1)
    print(h, str.repr(Some(copy(&h.item)?))?)?
    origin = Point(5, 6)
    shifted = def [origin](s: &Span) -> i64:
        s.lo.x + origin.x
    print(shifted(&h.item))?
    sender, receiver = channel[Tagged](2)?
    sender.send(Tagged(Span(Point(1, 2), Point(3, 4)), "sent"))?
    got = receiver.recv()?
    print(got)?
    with spawn(worker, got)? as task:
        back = task.join()
        print(back.span, back.name)?
    first = tags.pop()
    match first:
        case Some(t):
            print(t.span.lo)?
        case Nothing:
            pass
    Ok(())
"#,
        "t3 -2\nt2 2\nt1 6\nt0 10\n3 Span(lo=Point(x=0, y=0), hi=Point(x=1, y=2)) True\n6\n50\nempty\nHolder[Span](item=Span(lo=Point(x=1, y=2), hi=Point(x=3, y=4)), count=1) Option[Span].Some(Span(lo=Point(x=1, y=2), hi=Point(x=3, y=4)))\n6\nTagged(span=Span(lo=Point(x=1, y=2), hi=Point(x=3, y=4)), name=\"sent\")\nSpan(lo=Point(x=3, y=4), hi=Point(x=1, y=2)) sent\nPoint(x=0, y=0)\n",
    );
}

#[test]
fn nested_class_handles_cross_c_unchanged() {
    // A C handle still owns a boxed slot; the flat inner record is reached by
    // offset on both sides of the boundary, and the header does not change.
    let source = r#"
class Inner:
    text: str
    count: i64
class Outer:
    inner: Inner
    tag: i64
def repeated(count: i64) -> Result[str, AllocError]:
    mut text = ""
    for _ in range(count):
        text = (text + "x")?
    Ok(text)
export def create(count: i64) -> Result[Outer, AllocError] = "calc_create":
    Ok(Outer(Inner(repeated(count)?, count), count * 10))
export def length(a: &Outer, b: &Outer) -> i64 = "calc_length":
    len(a.inner.text) + len(b.inner.text) + a.tag
export def rename(owner: &mut Outer, count: i64) -> Result[(), AllocError] = "calc_rename":
    owner.inner.text = repeated(count)?
    owner.inner.count = count
    Ok(())
export def inner(owner: Outer) -> Result[Inner, AllocError] = "calc_inner":
    Ok(owner.inner)
export def inner_count(inner: &Inner) -> i64 = "calc_inner_count":
    inner.count + len(inner.text)
export def consume(owner: Outer) -> i64 = "calc_consume":
    inner = owner.inner
    len(inner.text) + owner.tag
export def identity(owner: Outer) -> Result[Outer, AllocError] = "calc_identity":
    Ok(owner)
"#;
    for kind in [plenty::LibraryKind::Static, plenty::LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let path = root.join("source.plenty");
        std::fs::write(&path, source).unwrap();
        let library = root.join(if kind == plenty::LibraryKind::Static {
            "libcalc.a"
        } else {
            "libcalc.so"
        });
        let options = plenty::LibraryOptions::new("calc", kind);
        let artifacts = plenty::compile_file_to_library(&path, &library, None, &options).unwrap();
        let header = std::fs::read_to_string(&artifacts.header).unwrap();
        assert!(header.contains("const calc_Inner * p0"), "{header}");
        let c = root.join("nested.c");
        std::fs::write(
            &c,
            r#"
#include "calc.h"
#include <assert.h>
int main(void) {
    calc_Outer *owner = 0;
    uint32_t error = 99;
    assert(calc_create(4, &owner, &error) == 0);
    assert(calc_length(owner, owner) == 48);
    assert(calc_rename(owner, 2, &error) == 0);
    assert(calc_length(owner, owner) == 44);
    calc_Outer *returned = 0;
    assert(calc_identity(owner, &returned, &error) == 0 && returned != 0);
    calc_Inner *inner = 0;
    assert(calc_inner(returned, &inner, &error) == 0 && inner != 0);
    assert(calc_inner_count(inner) == 4);
    calc_Inner_destroy(inner);
    assert(calc_create(3, &owner, &error) == 0);
    assert(calc_consume(owner) == 33);
    assert(calc_create(7, &owner, &error) == 0);
    calc_Outer_destroy(owner);
    return 0;
}
"#,
        )
        .unwrap();
        let executable = root.join("caller");
        let args = std::fs::read_to_string(artifacts.link_args).unwrap();
        for compiler in ["cc", "c++"] {
            let output = std::process::Command::new(compiler)
                .args(["-Wall", "-Wextra", "-Werror"])
                .arg(&c)
                .arg(&library)
                .args(args.lines())
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let run = std::process::Command::new(&executable).output().unwrap();
            assert!(
                run.status.success(),
                "{}",
                String::from_utf8_lossy(&run.stderr)
            );
        }
        let app = root.join("main.plenty");
        std::fs::write(
            &app,
            "import calc\ndef main() -> Result[(), AllocError | IoError]:\n    mut owner = calc.create(4)?\n    calc.rename(&mut owner, 2)?\n    print(calc.length(&owner, &owner))?\n    inner = calc.inner(calc.identity(owner)?)?\n    print(calc.inner_count(&inner))?\n    Ok(())\n",
        )
        .unwrap();
        let options = plenty::CompileOptions {
            link_args: vec![library.into_os_string()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&app, &executable, None, &options).unwrap();
        let run = std::process::Command::new(&executable).output().unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"44\n4\n");
    }
}
