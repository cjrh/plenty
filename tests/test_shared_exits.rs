//! A function's `?` failures and returns share their cleanup and release code
//! (issue #407): code grows linearly with the number of exits and live owned
//! locals, and every exit still drops in scope-exit order.
mod support;

/// Machine-code bytes of `main` in a program with `n` pairs of owned locals,
/// each followed by a `?` and kept live to the end. The strings are borrowed,
/// so they live in the frame; the lists stay in variables.
fn main_bytes(n: usize) -> usize {
    let mut source = String::from(
        "def show(text: &str) -> Result[(), IoError]:\n    print(text)\n\
         def main() -> Result[(), AllocError | IoError]:\n",
    );
    for i in 0..n {
        source.push_str(&format!(
            "    t{i} = \"item\".concat(\"-{i}\")?\n    show(&t{i})?\n    l{i} = [{i}]?\n    print(len(&l{i}))?\n"
        ));
    }
    source.push_str("    Ok(())\n");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), source).unwrap();
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args([
            "--dump",
            "asm",
            "--dump-filter",
            "main",
            "p.plenty",
            "-o",
            "out",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let index = std::fs::read_to_string(dir.path().join("out/index.txt")).unwrap();
    let row = index
        .lines()
        .find(|line| line.ends_with(" __plenty_fn_main main"))
        .unwrap_or_else(|| panic!("main not dumped:\n{index}"));
    row.split_whitespace().next().unwrap().parse().unwrap()
}

#[test]
fn exit_code_grows_linearly_with_live_locals() {
    // Lowering each exit's cleanup inline made 40 pairs 3.7 times the size
    // of 20; shared exits make it 2.0.
    let (small, large) = (main_bytes(20), main_bytes(40));
    assert!(
        large * 10 <= small * 22,
        "20 pairs: {small} bytes, 40 pairs: {large} bytes"
    );
}

#[test]
fn every_exit_drops_in_scope_order() {
    let output = support::run(
        r#"class Guard:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()

class Scope:
    name: str
    def __enter__(self: &mut Scope) -> ():
        print("enter", self.name).unwrap()
    def __exit__(self: &mut Scope) -> ():
        print("exit", self.name).unwrap()

def step(n: i64, fail: i64) -> Option[i64]:
    if n == fail:
        return Nothing
    Some(n)

def keep(g: Guard) -> ():
    print("keep", g.name).unwrap()

def run(fail: i64) -> Option[i64]:
    a = Guard("a")
    x = step(0, fail)?
    b = Guard("b")
    y = step(1, fail)?
    with Scope("outer"):
        c = Guard("c")
        z = step(2, fail)?
        with Scope("inner"):
            d = Guard("d")
            w = step(3, fail)?
            print(z + w).unwrap()
            if fail == 4:
                return Some(-1)
        e = Guard("e")
        v = step(5, fail)?
        print(v + len(c.name) + len(e.name)).unwrap()
    moved = Guard("moved")
    keep(moved)
    f = Guard("f")
    u = step(6, fail)?
    Some(x + y + u + len(a.name) + len(b.name) + len(f.name))

def main() -> ():
    for fail in range(8):
        print("fail at", fail).unwrap()
        match run(fail):
            case Some(n):
                print("result", n).unwrap()
            case Nothing:
                print("nothing").unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = [
        "fail at 0|a|nothing",
        "fail at 1|b|a|nothing",
        "fail at 2|enter outer|c|exit outer|b|a|nothing",
        "fail at 3|enter outer|enter inner|d|exit inner|c|exit outer|b|a|nothing",
        "fail at 4|enter outer|enter inner|5|d|exit inner|c|exit outer|b|a|result -1",
        "fail at 5|enter outer|enter inner|5|d|exit inner|e|c|exit outer|b|a|nothing",
        "fail at 6|enter outer|enter inner|5|d|exit inner|7|e|c|exit outer|keep moved|moved|f|b|a|nothing",
        "fail at 7|enter outer|enter inner|5|d|exit inner|7|e|c|exit outer|keep moved|moved|f|b|a|result 10",
    ]
    .join("|")
        + "|";
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\n', "|"),
        expected
    );
}
