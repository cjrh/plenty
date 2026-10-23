//! A function's `?` failures, returns, tail calls and `break`s share their
//! cleanup and release code (issues #407 and #417): code grows linearly with
//! the number of exits and live owned locals, and every exit still drops in
//! scope-exit order.
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
    function_bytes(&source, "main")
}

const SCOPE: &str = r#"class Scope:
    name: str
    def __enter__(self: &mut Scope) -> ():
        print("enter", self.name).unwrap()
    def __exit__(self: &mut Scope) -> ():
        print("exit", self.name).unwrap()

def finish(stop: i64) -> Result[(), AllocError | IoError]:
    print(stop)?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    run(3)?
    Ok(())

def run(stop: i64) -> Result[(), AllocError | IoError]:
"#;

/// Machine-code bytes of `run` in a program with `n` owned locals, each
/// followed by `exit`, taken when `stop` names the local. A nonempty
/// `around` opens the block holding them. The strings are borrowed, so they
/// live in the frame.
fn exits_bytes(n: usize, around: &str, exit: &str) -> usize {
    let mut source = String::from(SCOPE);
    let indent = if around.is_empty() {
        "    "
    } else {
        "        "
    };
    if !around.is_empty() {
        source.push_str(&format!("    {around}\n"));
    }
    for i in 0..n {
        source.push_str(&format!(
            "{indent}t{i} = \"item\".concat(\"-{i}\")?\n{indent}print(&t{i})?\n\
             {indent}if stop == {i}:\n{indent}    {exit}\n"
        ));
    }
    if around.starts_with("while") {
        source.push_str(&format!("{indent}break\n"));
    }
    source.push_str("    Ok(())\n");
    function_bytes(&source, "run")
}

/// Asserts that 40 exits take at most 2.2 times the code of 20.
fn assert_linear(around: &str, exit: &str) {
    let (small, large) = (exits_bytes(20, around, exit), exits_bytes(40, around, exit));
    assert!(
        large * 10 <= small * 22,
        "`{exit}` in `{around}`: 20 exits: {small} bytes, 40 exits: {large} bytes"
    );
}

/// Machine-code bytes of the function `name` in `source`.
fn function_bytes(source: &str, name: &str) -> usize {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), source).unwrap();
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args([
            "--dump",
            "asm",
            "--dump-filter",
            name,
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
    let suffix = format!(" __plenty_fn_{name} {name}");
    let row = index
        .lines()
        .find(|line| line.ends_with(&suffix))
        .unwrap_or_else(|| panic!("{name} not dumped:\n{index}"));
    row.split_whitespace().next().unwrap().parse().unwrap()
}

#[test]
fn returns_in_a_context_grow_linearly() {
    // Lowering each return's scope cleanup inline made 40 returns 5.6 times
    // the size of 20; shared exits make it 2.0.
    assert_linear("with Scope(\"s\"):", "return Ok(())");
}

#[test]
fn breaks_grow_linearly() {
    // Inline: 2.9 times; shared: 2.0.
    assert_linear("while stop >= 0:", "break");
}

#[test]
fn tail_calls_grow_linearly() {
    // Inline: 3.2 times; shared: 2.0.
    assert_linear("", "return finish(stop)");
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

#[test]
fn returns_breaks_and_tail_calls_drop_in_scope_order() {
    // The borrowed guards live in the frame; `moved` leaves its slot empty.
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

def show(g: &Guard) -> ():
    print("show", g.name).unwrap()

def keep(g: Guard) -> ():
    print("keep", g.name).unwrap()

def returns(stop: i64) -> i64:
    a = Guard("a")
    show(&a)
    with Scope("outer"):
        b = Guard("b")
        if stop == 0:
            return 0
        with Scope("inner"):
            c = Guard("c")
            show(&c)
            if stop == 1:
                return 1
            d = Guard("d")
            if stop == 2:
                return 2
        e = Guard("e")
        if stop == 3:
            return 3
        moved = Guard("moved")
        keep(moved)
        if stop == 4:
            return 4
    f = Guard("f")
    if stop == 5:
        return 5
    6

def breaks(stop: i64) -> i64:
    a = Guard("a")
    mut n = 0
    for i in range(3):
        b = Guard("b")
        with Scope("loop"):
            c = Guard("c")
            show(&c)
            if i == stop:
                break
            d = Guard("d")
            for j in range(2):
                g = Guard("g")
                if i * 10 + j + 100 == stop:
                    break
                h = Guard("h")
            if i + 10 == stop:
                break
        e = Guard("e")
        show(&e)
        if i + 20 == stop:
            break
        n += 1
    f = Guard("f")
    n

def finish(n: i64) -> i64:
    print("finish", n).unwrap()
    n

def tails(stop: i64) -> i64:
    a = Guard("a")
    show(&a)
    if stop == 0:
        return finish(0)
    b = Guard("b")
    if stop == 1:
        return finish(1)
    c = Guard("c")
    show(&c)
    if stop == 2:
        return finish(2)
    finish(3)

def main() -> ():
    for stop in range(7):
        print("returns", stop).unwrap()
        print("result", returns(stop)).unwrap()
    for stop in [0, 10, 21, 100, 111, 99].unwrap():
        print("breaks", stop).unwrap()
        print("result", breaks(stop)).unwrap()
    for stop in range(4):
        print("tails", stop).unwrap()
        print("result", tails(stop)).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Lowering each exit inline printed the same.
    let expected = [
        "returns 0|show a|enter outer|b|exit outer|a|result 0",
        "returns 1|show a|enter outer|enter inner|show c|c|exit inner|b|exit outer|a|result 1",
        "returns 2|show a|enter outer|enter inner|show c|d|c|exit inner|b|exit outer|a|result 2",
        "returns 3|show a|enter outer|enter inner|show c|d|c|exit inner|e|b|exit outer|a|result 3",
        "returns 4|show a|enter outer|enter inner|show c|d|c|exit inner|keep moved|moved|e|b|exit outer|a|result 4",
        "returns 5|show a|enter outer|enter inner|show c|d|c|exit inner|keep moved|moved|e|b|exit outer|f|a|result 5",
        "returns 6|show a|enter outer|enter inner|show c|d|c|exit inner|keep moved|moved|e|b|exit outer|f|a|result 6",
        "breaks 0|enter loop|show c|c|exit loop|b|f|a|result 0",
        "breaks 10|enter loop|show c|h|g|h|g|d|c|exit loop|b|f|a|result 0",
        "breaks 21|enter loop|show c|h|g|h|g|d|c|exit loop|show e|e|b|enter loop|show c|h|g|h|g|d|c|exit loop|show e|e|b|f|a|result 1",
        "breaks 100|enter loop|show c|g|d|c|exit loop|show e|e|b|enter loop|show c|h|g|h|g|d|c|exit loop|show e|e|b|enter loop|show c|h|g|h|g|d|c|exit loop|show e|e|b|f|a|result 3",
        "breaks 111|enter loop|show c|h|g|h|g|d|c|exit loop|show e|e|b|enter loop|show c|h|g|g|d|c|exit loop|show e|e|b|enter loop|show c|h|g|h|g|d|c|exit loop|show e|e|b|f|a|result 3",
        "breaks 99|enter loop|show c|h|g|h|g|d|c|exit loop|show e|e|b|enter loop|show c|h|g|h|g|d|c|exit loop|show e|e|b|enter loop|show c|h|g|h|g|d|c|exit loop|show e|e|b|f|a|result 3",
        "tails 0|show a|a|finish 0|result 0",
        "tails 1|show a|b|a|finish 1|result 1",
        "tails 2|show a|show c|c|b|a|finish 2|result 2",
        "tails 3|show a|show c|c|b|a|finish 3|result 3",
    ]
    .join("|")
        + "|";
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\n', "|"),
        expected
    );
}
