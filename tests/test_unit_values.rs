//! Unit `()` is an ordinary value type (issue #870): it binds, passes, stores
//! and instantiates generics like any other copyable value.

mod support;

#[test]
fn unit_binds_passes_and_formats() {
    support::assert_stdout(
        r#"def take(value: ()) -> ():
    value

def identity[T](value: T) -> T:
    value

def twice[T](value: T) -> (T, T):
    (value, value)

def main() -> Result[(), AllocError | IoError]:
    marker = ()
    annotated: () = marker
    mut later = ()
    later = take(annotated)
    print(marker, later, identity[()](()), identity(()))?
    print(marker == (), later != ())?
    print(f"{marker}"?, str.repr(twice(marker))?)?
    reference = &marker
    print(*reference)?
    first, second = twice(())
    (nested, count) = ((first, second), 2)
    print(str.repr(nested)?, count)?
    pair = ((), 3)
    copied = pair[0]
    print(copied, pair[1], ((), 1)[0])?
    result = print("done")?
    print(result)?
    Ok(())
"#,
        "() () () ()\nTrue False\n() ((), ())\n()\n((), ()) 2\n() 3 ()\ndone\n()\n",
    );
}

#[test]
fn unit_instantiates_user_generic_data_and_collections() {
    support::assert_stdout(
        r#"enum Slot[T]:
    Full(T)
    Empty

class Pair[A, B]:
    first: A
    second: B

class Done:
    marker: ()

def wrap[T](value: T) -> Slot[T]:
    Slot[T].Full(value)

def main() -> Result[(), AllocError | IoError]:
    pair = Pair[(), Option[()]]((), Some(()))
    print(str.repr(pair)?)?
    print(Done(()).marker)?
    nested: Slot[Slot[()]] = Slot[Slot[()]].Full(wrap(()))
    match nested:
        case Slot.Full(Slot.Full(inner)):
            print("full", inner)?
        case _:
            print("other")?
    make = Slot[()].Full
    print(str.repr(make(()))?)?
    mut signals: list[()] = [(), ()]?
    signals.append(())?
    for signal in &signals:
        print(signal)?
    print(len(signals), str.repr(signals.pop())?)?
    names: dict[str, ()] = {"stop": ()}?
    print(str.repr(names)?)?
    ok: Result[(), str] = Ok(())
    print(str.repr(Some(ok))?)?
    boxed = Box(())?
    print(*boxed)?
    shared = Shared(())?
    view = shared.share()
    print(*view)?
    Ok(())
"#,
        "Pair[(), Option[()]](first=(), second=Option[()].Some(()))\n\
         ()\n\
         full ()\n\
         Slot[()].Full(())\n\
         ()\n()\n()\n\
         3 Option[()].Some(())\n\
         {\"stop\": ()}\n\
         Option[Result[(), str]].Some(Result[(), str].Ok(()))\n\
         ()\n()\n",
    );
}

#[test]
fn unit_flows_through_callables_generators_and_protocols() {
    support::assert_stdout(
        r#"protocol Sink[T]:
    def put(self: &mut Sink, value: T) -> ():
        pass

class Counter:
    count: i64

    def put(self: &mut Counter, value: ()) -> ():
        unchecked:
            self.count = self.count + 1

def feed[S: Sink[()]](sink: &mut S) -> ():
    sink.put(())

def apply[T, F: Callable[[T], T]](f: &F, value: T) -> T:
    f(value)

def repeat[T](value: T, n: i64) -> Generator[T]:
    for _ in range(n):
        yield value

def make() -> Callable[[()], i64]:
    (def(u: ()) -> i64: 7)

def main() -> Result[(), AllocError | IoError]:
    mut counter = Counter(0)
    feed(&mut counter)
    counter.put(())
    print(counter.count)?
    print(apply(&(def(u: ()) -> (): u), ()))?
    f = make()
    g = identity[()]
    print(f(()), g(()))?
    n = 1
    captured = (def [n](u: ()) -> i64: n)
    print(captured(()))?
    for u in repeat((), 2):
        print(u)?
    print(str.repr(list(repeat((), 3))?)?)?
    Ok(())

def identity[T](value: T) -> T:
    value
"#,
        "2\n()\n7 ()\n1\n()\n()\n[(), (), ()]\n",
    );
}

#[test]
fn unit_keeps_key_option_and_exit_rules() {
    for (source, expected) in [
        ("s: set[()] = {()}.unwrap()", "not a supported key type"),
        (
            "d: dict[(), i64] = {(): 1}.unwrap()",
            "not a supported key type",
        ),
        ("x = None", "Plenty has no `None`"),
        ("x: () = 1", "expected (), got i64"),
        ("x: i64 = ()", "expected i64, got ()"),
        (
            "u = ()\nmatch u:\n    case 1:\n        pass",
            "an integer pattern cannot match a value of type ()",
        ),
        ("mut x = ()\nx = 1", "expected (), got i64"),
        (
            "def f(flag: bool) -> i64:\n    x = match flag:\n        case True:\n            return 1\n        case False:\n            return 2\n    x",
            "every case of this `match` exits, so it has no value",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{source}: {error}");
    }
}

// Review of PR #1104: captures, generator expressions, `match` and `fold` read
// a unit value as an operand, including at `T = ()`.
#[test]
fn unit_is_captured_matched_and_folded() {
    support::assert_stdout(
        r#"class Done:
    marker: ()

def cap[T](x: T) -> T:
    f = (def [x]() -> T: x)
    f()

def kind[T](x: T) -> i64:
    match x:
        case _:
            1

def same[T](x: T) -> T:
    match x:
        case _ as y:
            y

def marker(d: &Done) -> ():
    d.marker

def first(t: ((), i64)) -> ():
    t[0]

def step(acc: (), item: &()) -> Result[(), str]:
    Ok(acc)

def main() -> Result[(), AllocError | IoError]:
    u = ()
    print(cap(()), cap(u), cap(1), kind(u), same(()))?
    f = (def [u]() -> (): u)
    print(f())?
    for v in (u for _ in range(2)):
        print(v)?
    match u:
        case _ as w:
            print("matched", w)?
    d = Done(())
    print(marker(&d), first(((), 1)), str.repr(Some(d.marker))?)?
    xs = [(), ()]?
    print(iter(&xs).fold((), def(acc: (), item: &()) -> (): acc))?
    print(iter(&xs).fold(u, def(acc: (), item: &()) -> (): acc))?
    print(str.repr(iter(&xs).try_fold((), step))?)?
    Ok(())
"#,
        "() () 1 1 ()\n()\n()\n()\nmatched ()\n() () Option[()].Some(())\n()\n()\nResult[(), str].Ok(())\n",
    );
}
