//! Exercise generated-code cleanup, including memory retained across suspension.
//! Run with --features runtime-checks to count every Rust runtime allocation.
#![cfg(unix)]
use std::process::Command;

#[test]
fn native_owners_are_reclaimed() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("ownership.plenty");
    std::fs::write(
        &source,
        r#"
enum Data:
    Values(list[i64])

def suspended() -> Generator[i64]:
    len([n for n in range(10_000).unwrap()].unwrap())
    match Data.Values([n for n in range(10_000).unwrap()].unwrap()).unwrap():
        case Data.Values(values):
            len(values)
    yield 1
    yield 2

def captured(values: list[str]) -> Generator[str]:
    for value in values:
        yield value

def release_unstarted() -> ():
    captured([('a' + 'b').unwrap() for n in range(100).unwrap()].unwrap()).unwrap()
    pass

def early() -> str:
    for value in captured([('x' + 'y').unwrap()].unwrap()).unwrap():
        return value
    "empty"

def tail(n: i64, text: str) -> str:
    if n == 0:
        return text
    tail(n - 1, (text + "").unwrap())
def wrapper(child: Generator[str]) -> Generator[str]:
    for value in child:
        yield value
class Buffer:
    contents: list[str]
    def __del__(self) -> ():
        self.contents.append(("cleanup" + " value").unwrap()).unwrap()
class Pair:
    first: Buffer
    second: Buffer
def buffers(a: Buffer, b: Buffer) -> Generator[Buffer]:
    local = Buffer([("local" + " data").unwrap()].unwrap()).unwrap()
    yield a
    yield b

def main() -> ():
    mut it = suspended().unwrap()
    next(it)
    print("__test_small_live_heap__").unwrap()
    next(it)
    next(it)
    next(it)
    print("__test_small_live_heap__").unwrap()
    release_unstarted()
    owned = [n for n in range(10_000).unwrap()].unwrap()
    drop(owned)
    abandoned = captured([('drop' + ' capture').unwrap() for n in range(100).unwrap()].unwrap()).unwrap()
    drop(abandoned)
    print("__test_small_live_heap__").unwrap()
    print(early()).unwrap()
    for value in captured([('c' + 'd').unwrap() for n in range(100).unwrap()].unwrap()).unwrap():
        break
    print("__test_small_live_heap__").unwrap()

    mut text = ""
    for n in range(500).unwrap():
        text = (text + "é\0").unwrap()
        mut values = {text: [text].unwrap()}.unwrap()
        saved = copy(values).unwrap()
        values[text] = [('new' + ' value').unwrap()].unwrap()
        len(saved[text])

    print(len(tail(10_000, text))).unwrap()

    wrapper(captured([('nested' + ' frame').unwrap()].unwrap()).unwrap()).unwrap()
    mut finished = wrapper(captured([('one' + ' value').unwrap()].unwrap()).unwrap()).unwrap()
    next(finished)
    next(finished)
    next(finished)

    for n in range(100).unwrap():
        drop(Pair(Buffer([("a" + " data").unwrap()].unwrap()).unwrap(), Buffer([("b" + " data").unwrap()].unwrap()).unwrap()).unwrap())
        mut iterator = buffers(Buffer([("x" + " data").unwrap()].unwrap()).unwrap(), Buffer([("y" + " data").unwrap()].unwrap()).unwrap()).unwrap()
        next(iterator)
        drop(iterator)
        drop(buffers(Buffer([("unstarted" + " data").unwrap()].unwrap()).unwrap(), Buffer([("unused" + " data").unwrap()].unwrap()).unwrap()).unwrap())
        for item in [Buffer([("first" + " data").unwrap()].unwrap()).unwrap(), Buffer([("second" + " data").unwrap()].unwrap()).unwrap()].unwrap():
            drop(item)
        match Option[Buffer].Some(Buffer([("matched" + " data").unwrap()].unwrap()).unwrap()):
            case Option[Buffer].Some(value):
                drop(value)
            case Option[Buffer].Nothing:
                pass
"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg(source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "__test_small_live_heap__\n__test_small_live_heap__\n__test_small_live_heap__\nxy\n__test_small_live_heap__\n1000\n"
    );
}
