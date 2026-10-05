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
    len([n for n in range(10_000)])
    match Data.Values([n for n in range(10_000)]):
        case Data.Values(values):
            len(values)
    yield 1
    yield 2

def captured(values: list[str]) -> Generator[str]:
    for value in values:
        yield value

def release_unstarted() -> ():
    captured(['a' + 'b' for n in range(100)])
    pass

def early() -> str:
    for value in captured(['x' + 'y']):
        return value
    "empty"

def tail(n: i64, text: str) -> str:
    if n == 0:
        return text
    tail(n - 1, text + "")
def wrapper(child: Generator[str]) -> Generator[str]:
    for value in child:
        yield value
class Buffer:
    contents: list[str]
    def __del__(self) -> ():
        self.contents.append("cleanup" + " value")
class Pair:
    first: Buffer
    second: Buffer
def buffers(a: Buffer, b: Buffer) -> Generator[Buffer]:
    local = Buffer(["local" + " data"])
    yield a
    yield b

def main() -> ():
    mut it = suspended()
    next(it)
    print("__test_small_live_heap__")
    next(it)
    next(it)
    next(it)
    print("__test_small_live_heap__")
    release_unstarted()
    owned = [n for n in range(10_000)]
    drop(owned)
    abandoned = captured(['drop' + ' capture' for n in range(100)])
    drop(abandoned)
    print("__test_small_live_heap__")
    print(early())
    for value in captured(['c' + 'd' for n in range(100)]):
        break
    print("__test_small_live_heap__")

    mut text = ""
    for n in range(500):
        text = text + "é\0"
        mut values = {text: [text]}
        saved = copy(values)
        values[text] = ['new' + ' value']
        len(saved[text])

    print(len(tail(10_000, text)))

    wrapper(captured(['nested' + ' frame']))
    mut finished = wrapper(captured(['one' + ' value']))
    next(finished)
    next(finished)
    next(finished)

    for n in range(100):
        drop(Pair(Buffer(["a" + " data"]), Buffer(["b" + " data"])))
        mut iterator = buffers(Buffer(["x" + " data"]), Buffer(["y" + " data"]))
        next(iterator)
        drop(iterator)
        drop(buffers(Buffer(["unstarted" + " data"]), Buffer(["unused" + " data"])))
        for item in [Buffer(["first" + " data"]), Buffer(["second" + " data"])]:
            drop(item)
        match Option[Buffer].Some(Buffer(["matched" + " data"])):
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
