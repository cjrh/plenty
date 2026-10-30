//! A class component of a tuple or multi-field variant is stored flat, as a
//! class field of a class is (issue #551): its record bytes sit at a fixed
//! offset in the owner's record, with no slot word holding its address, so
//! moving such a tuple or variant copies bytes without repairing addresses.
//! Single-field variants, such as `Option` and `Result` payloads, keep their
//! address word. Matching, payload loans, moves, destructors, copies,
//! equality, formatting and every kind of holder behave as before.
mod support;
use support::assert_visible_stdout as native;

fn plenty(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

/// The assembly of the function whose dump section name starts with `name`.
fn section(text: &str, name: &str) -> String {
    let start = text
        .find(&format!("==== {name}"))
        .unwrap_or_else(|| panic!("{name} not dumped"));
    let rest = &text[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

/// Address fixes after a copy: `lea OFF+16(%base),%r` then `mov %r,OFF(%base)`,
/// pointing a slot's word at the payload that follows it.
#[cfg(not(target_arch = "aarch64"))]
fn address_fixes(asm: &str) -> usize {
    let operands: Vec<Vec<&str>> = asm
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(_, ins)| ins.split_whitespace().collect())
        .collect();
    let place = |operand: &str| -> Option<(i64, String)> {
        let (offset, base) = operand.split_once('(')?;
        let offset = match offset.strip_prefix("0x") {
            Some(hex) => i64::from_str_radix(hex, 16).ok()?,
            None if offset.is_empty() => 0,
            None => offset.parse().ok()?,
        };
        Some((offset, base.trim_end_matches(')').to_string()))
    };
    operands
        .windows(2)
        .filter(|pair| {
            let ([lea, from], [mov, to]) = (&pair[0][..], &pair[1][..]) else {
                return false;
            };
            if *lea != "lea" || *mov != "mov" {
                return false;
            }
            let (Some((source, register)), Some((target, slot))) = (
                from.split_once(',').map(|(a, r)| (place(a), r)),
                to.split_once(','),
            ) else {
                return false;
            };
            let (Some((payload, base)), Some((word, owner))) = (source, place(slot)) else {
                return false;
            };
            target == register && base == owner && payload == word + 16
        })
        .count()
}

/// AArch64's corresponding `add dst,base,#payload; str dst,[base,#word]`.
#[cfg(target_arch = "aarch64")]
fn address_fixes(asm: &str) -> usize {
    let instructions: Vec<Vec<&str>> = asm
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(_, instruction)| instruction.split_whitespace().collect())
        .collect();
    let number = |text: &str| -> Option<i64> {
        let text = text.trim_start_matches('#');
        text.strip_prefix("0x").map_or_else(
            || text.parse().ok(),
            |hex| i64::from_str_radix(hex, 16).ok(),
        )
    };
    instructions
        .windows(2)
        .filter(|pair| {
            let ["add", destination, base, payload] = pair[0].as_slice() else {
                return false;
            };
            let [store, register, memory @ ..] = pair[1].as_slice() else {
                return false;
            };
            if !matches!(*store, "str" | "stur")
                || destination.trim_end_matches(',') != register.trim_end_matches(',')
            {
                return false;
            }
            let memory = memory.join(" ");
            let Some(memory) = memory
                .strip_prefix('[')
                .and_then(|text| text.strip_suffix(']'))
            else {
                return false;
            };
            let (owner, offset) = memory
                .split_once(',')
                .map_or((memory, Some(0)), |(owner, offset)| {
                    (owner, number(offset.trim()))
                });
            base.trim_end_matches(',') == owner
                && number(payload)
                    .zip(offset)
                    .is_some_and(|(payload, word)| payload == word + 16)
        })
        .count()
}

const LAYOUTS: &str = r#"class Point:
    x: i64
    y: i64

enum Shape:
    Empty
    Seg(Point, Point)
    Tagged(Point, i64)

def keep(t: (Point, Point)) -> (Point, Point):
    t

def swap(t: (Point, Point)) -> (Point, Point):
    a, b = t
    (b, a)

def keep_shape(s: Shape) -> Shape:
    s

def keep_some(t: (Option[Point], i64)) -> (Option[Point], i64):
    t

def main() -> Result[(), AllocError | IoError]:
    print(swap(keep((Point(1, 2), Point(3, 4)))), keep_shape(Shape.Seg(Point(5, 6), Point(7, 8))))?
    print(keep_some((Some(Point(9, 9)), 1)))?
    Ok(())
"#;

#[test]
fn flat_payloads_copy_without_address_fixes() {
    native(
        LAYOUTS,
        "(Point(x=3, y=4), Point(x=1, y=2)) Shape.Seg(Point(x=5, y=6), Point(x=7, y=8))\n(Option[Point].Some(Point(x=9, y=9)), 1)\n",
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), LAYOUTS).unwrap();
    let args = ["--release", "--dump", "asm", "p.plenty", "-o", "out"];
    let output = plenty(dir.path(), &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(dir.path().join("out").join("asm.txt")).unwrap();
    for name in ["keep ", "swap ", "keep_shape "] {
        let asm = section(&text, name);
        assert_eq!(address_fixes(&asm), 0, "{name}:\n{asm}");
        for call in ["%Memmove", "%Memcpy", "# plenty_op_117"] {
            assert!(!asm.contains(call), "{name} calls {call}:\n{asm}");
        }
    }
    // An `Option` component keeps its address word, which each copy fixes.
    let asm = section(&text, "keep_some ");
    assert!(address_fixes(&asm) > 0, "keep_some:\n{asm}");
}

#[test]
fn flat_payloads_match_borrow_write_move_copy_and_compare() {
    native(
        r#"class Point:
    x: i64
    y: i64

class Label:
    text: str
    point: Point

enum Shape:
    Empty
    Seg(Point, Point)
    Named(Label, i64, Point)

def bump(p: &mut Point, by: i64) -> ():
    p.x += by
    p.y += by

def keep(t: (Point, Label)) -> (Point, Label):
    t

def swap(t: (Point, Point)) -> (Point, Point):
    a, b = t
    (b, a)

def measure(s: &Shape) -> i64:
    match s:
        case Shape.Seg(a, b):
            b.x - a.x + b.y - a.y
        case Shape.Named(l, n, p):
            l.point.x + *n + p.y
        case Shape.Empty:
            0

def consume(s: Shape) -> Result[str, AllocError]:
    match s:
        case Shape.Seg(a, b):
            Ok(str.from(a.x + b.x)?)
        case Shape.Named(l, n, p):
            Ok((l.text + str.from(n + p.x)?)?)
        case Shape.Empty:
            Ok("empty")

def grow(s: &mut Shape) -> ():
    match s:
        case Shape.Seg(a, b):
            bump(a, 1)
            b.y = 100
        case Shape.Named(l, _, p):
            l.point = Point(-1, -2)
            bump(p, 5)
        case Shape.Empty:
            pass

def main() -> Result[(), AllocError | IoError]:
    t = keep((Point(1, 2), Label("one", Point(3, 4))))
    print(t)?
    mut u = swap((Point(1, 2), Point(3, 4)))
    bump(&mut u[0], 10)
    r = &mut u[1]
    r.x = 7
    r.y = 9
    print(u, u[1].x, u == (Point(13, 14), Point(7, 9)))?
    first, label = t
    print(first, label.text, label.point)?
    mut shapes: list[Shape] = [Shape.Seg(Point(0, 0), Point(3, 4)), Shape.Named(Label("n", Point(5, 6)), 7, Point(8, 9)), Shape.Empty]?
    for s in &shapes:
        print(measure(s))?
    for s in &mut shapes:
        grow(s)
    print(shapes)?
    copied = copy(&shapes)?
    print(copied == shapes, str.repr(copied[1])?)?
    while len(shapes) > 0:
        s = shapes.pop().unwrap()
        print(consume(s)?)?
    mut pairs: dict[str, (Point, Label)] = {}?
    pairs.insert("a", (Point(1, 1), Label("x", Point(2, 2))))?
    pairs.insert("b", (Point(3, 3), Label("y", Point(4, 4))))?
    print(pairs["b"], len(pairs))?
    Ok(())
"#,
        "(Point(x=1, y=2), Label(text=\"one\", point=Point(x=3, y=4)))\n(Point(x=13, y=14), Point(x=7, y=9)) 7 True\nPoint(x=1, y=2) one Point(x=3, y=4)\n7\n21\n0\n[Shape.Seg(Point(x=1, y=1), Point(x=3, y=100)), Shape.Named(Label(text=\"n\", point=Point(x=-1, y=-2)), 7, Point(x=13, y=14)), Shape.Empty]\nTrue Shape.Named(Label(text=\"n\", point=Point(x=-1, y=-2)), 7, Point(x=13, y=14))\nempty\nn20\n4\n(Point(x=3, y=3), Label(text=\"y\", point=Point(x=4, y=4))) 2\n",
    );
}

#[test]
fn flat_payloads_keep_destructor_order_across_holders() {
    // Moved-out components are released once; the rest of a consumed tuple or
    // variant in field order. Generators, closures, channels and threads hold
    // tuples of records.
    native(
        r#"class Guard:
    name: str
    def __del__(self) -> ():
        print("drop", self.name).unwrap()

class Point:
    x: i64
    y: i64

class Pair:
    left: Guard
    right: Guard

enum Event:
    Quiet
    Two(Guard, Point, Guard)
    Nested(Pair, Guard)

class Holder[T]:
    item: T
    count: i64

def left_of(e: Event) -> Result[str, AllocError]:
    match e:
        case Event.Two(a, _, _):
            Ok(a.name)
        case Event.Nested(p, g):
            drop(g)
            Ok(p.right.name)
        case Event.Quiet:
            Ok("quiet")

def points(n: i64) -> Generator[(Point, Point)]:
    for i in range(n):
        yield (Point(i, i), Point(-i, i * 2))

def worker(t: (Point, Point, i64)) -> (Point, Point, i64):
    a, b, n = t
    (b, a, n * 2)

def main() -> Result[(), AllocError | IoError | ThreadError | ChannelError | SendError[(Pair, Point)] | RecvError]:
    print(left_of(Event.Two(Guard("a"), Point(1, 2), Guard("b")))?)?
    print(left_of(Event.Nested(Pair(Guard("pl"), Guard("pr")), Guard("g")))?)?
    t = (Guard("t1"), Guard("t2"))
    _, kept = t
    print("kept", kept.name)?
    mut events: list[Event] = [Event.Two(Guard("x"), Point(0, 0), Guard("y")), Event.Quiet, Event.Nested(Pair(Guard("p1"), Guard("p2")), Guard("p3"))]?
    events[0] = Event.Quiet
    print("replaced")?
    h = Holder[(Point, Point)]((Point(1, 2), Point(3, 4)), 5)
    print(h)?
    mut total = 0
    for a, b in points(4):
        total += a.x + b.y
    print(total)?
    offset = (Point(10, 20), Point(1, 1))
    shift = def [offset](p: &Point) -> i64:
        p.x + offset[0].x + offset[1].y
    print(shift(&Point(1, 0)))?
    sender, receiver = channel[(Pair, Point)](1)?
    sender.send((Pair(Guard("c1"), Guard("c2")), Point(7, 8)))?
    got, at = receiver.recv()?
    print(got.left.name, at)?
    with spawn(worker, (Point(1, 2), Point(3, 4), 21))? as task:
        print(task.join())?
    print("end")?
    Ok(())
"#,
        "drop b\ndrop a\na\ndrop g\ndrop pl\ndrop pr\npr\ndrop t1\nkept t2\ndrop x\ndrop y\nreplaced\nHolder[tuple[Point, Point]](item=(Point(x=1, y=2), Point(x=3, y=4)), count=5)\n18\n12\nc1 Point(x=7, y=8)\n(Point(x=3, y=4), Point(x=1, y=2), 42)\nend\ndrop c1\ndrop c2\ndrop p1\ndrop p2\ndrop p3\ndrop t2\n",
    );
}
