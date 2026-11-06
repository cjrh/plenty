//! A function's `?` failures, returns, tail calls, `break`s and `continue`s
//! share their cleanup and release code (issues #407, #417 and #419): code
//! grows linearly with
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
/// followed by `exit`, taken when `stop` names the local; `{i}` in `exit`
/// names the local's index, and functions `to0` to `to{n-1}` take `stop`.
/// `around` opens the nested blocks holding them. The strings are borrowed,
/// so they live in the frame.
fn exits_bytes(n: usize, around: &[&str], exit: &str) -> usize {
    let mut source = String::from(SCOPE);
    let mut indent = String::from("    ");
    for line in around {
        source.push_str(&format!("{indent}{line}\n"));
        indent.push_str("    ");
    }
    for i in 0..n {
        let exit = exit.replace("{i}", &i.to_string());
        source.push_str(&format!(
            "{indent}t{i} = \"item\".concat(\"-{i}\")?\n{indent}print(&t{i})?\n\
             {indent}if stop == {i}:\n{indent}    {exit}\n"
        ));
    }
    if around.first().is_some_and(|line| line.starts_with("while")) {
        source.push_str(&format!("{indent}break\n"));
    }
    source.push_str("    Ok(())\n");
    for i in 0..n {
        source.push_str(&format!(
            "\ndef to{i}(stop: i64) -> Result[(), AllocError | IoError]:\n    print(stop, {i})?\n    Ok(())\n"
        ));
    }
    function_bytes(&source, "run")
}

/// Asserts that 40 exits take at most 2.2 times the code of 20.
fn assert_linear(around: &[&str], exit: &str) {
    let (small, large) = (exits_bytes(20, around, exit), exits_bytes(40, around, exit));
    assert!(
        large * 10 <= small * 22,
        "`{exit}` in {around:?}: 20 exits: {small} bytes, 40 exits: {large} bytes"
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
    assert_linear(&["with Scope(\"s\"):"], "return Ok(())");
}

#[test]
fn breaks_grow_linearly() {
    // Inline: 2.9 times; shared: 2.0.
    assert_linear(&["while stop >= 0:"], "break");
}

#[test]
fn continues_grow_linearly() {
    // Inline (issue #419): 2.6 times; shared: 1.9.
    assert_linear(&["for k in range(3):", "with Scope(\"s\"):"], "continue");
}

#[test]
fn tail_calls_grow_linearly() {
    // Inline: 3.2 times; shared: 2.0.
    assert_linear(&[], "return finish(stop)");
}

#[test]
fn tail_calls_to_different_callees_grow_linearly() {
    // A chain per callee (issue #419): 3.0 times; shared: 2.0.
    assert_linear(&[], "return to{i}(stop)");
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
            # step returns its first argument: these successful values are 2 and 3.
            total = unchecked:
                z + w
            print(total).unwrap()
            if fail == 4:
                return Some(-1)
        e = Guard("e")
        v = step(5, fail)?
        # v is step(5); both immutable guard names contain one scalar.
        total = unchecked:
            v + len(c.name) + len(e.name)
        print(total).unwrap()
    moved = Guard("moved")
    keep(moved)
    f = Guard("f")
    u = step(6, fail)?
    # Successful steps fix x=0, y=1, u=6; the three names have length one.
    total = unchecked:
        x + y + u + len(a.name) + len(b.name) + len(f.name)
    Some(total)

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

def breaks(stop: i64) -> Result[i64, OverflowError[i64]]:
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
    Ok(n)

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

def main() -> Result[(), OverflowError[i64]]:
    for stop in range(7):
        print("returns", stop).unwrap()
        print("result", returns(stop)).unwrap()
    for stop in [0, 10, 21, 100, 111, 99].unwrap():
        print("breaks", stop).unwrap()
        print("result", breaks(stop)?).unwrap()
    for stop in range(4):
        print("tails", stop).unwrap()
        print("result", tails(stop)).unwrap()
    Ok(())
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

#[test]
fn continues_and_tail_calls_to_different_callees_drop_in_scope_order() {
    // `keep` leaves `c`'s slot empty; `zero` and `one` share releases.
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

def continues(stop: i64) -> Result[i64, OverflowError[i64]]:
    mut n = 0
    for i in range(2):
        a = Guard("a")
        with Scope("loop"):
            b = Guard("b")
            show(&b)
            if i == stop:
                continue
            c = Guard("c")
            if i + 10 == stop:
                keep(c)
                continue
            for j in range(2):
                d = Guard("d")
                if i * 10 + j + 100 == stop:
                    continue
                e = Guard("e")
                show(&e)
            n += 1
        n += 10
    Ok(n)

def zero(n: i64) -> i64:
    print("zero", n).unwrap()
    n

def one(n: i64) -> i64:
    print("one", n).unwrap()
    n

def tails(stop: i64) -> i64:
    a = Guard("a")
    show(&a)
    if stop == 0:
        return zero(0)
    b = Guard("b")
    if stop == 1:
        return one(1)
    c = Guard("c")
    show(&c)
    if stop == 2:
        return zero(2)
    one(3)

def main() -> Result[(), OverflowError[i64]]:
    for stop in [0, 1, 10, 101, 99].unwrap():
        print("continues", stop).unwrap()
        print("result", continues(stop)?).unwrap()
    for stop in range(4):
        print("tails", stop).unwrap()
        print("result", tails(stop)).unwrap()
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Lowering each exit inline printed the same.
    let expected = [
        "continues 0|enter loop|show b|b|exit loop|a|enter loop|show b|show e|e|d|show e|e|d|c|b|exit loop|a|result 11",
        "continues 1|enter loop|show b|show e|e|d|show e|e|d|c|b|exit loop|a|enter loop|show b|b|exit loop|a|result 11",
        "continues 10|enter loop|show b|keep c|c|b|exit loop|a|enter loop|show b|show e|e|d|show e|e|d|c|b|exit loop|a|result 11",
        "continues 101|enter loop|show b|show e|e|d|d|c|b|exit loop|a|enter loop|show b|show e|e|d|show e|e|d|c|b|exit loop|a|result 22",
        "continues 99|enter loop|show b|show e|e|d|show e|e|d|c|b|exit loop|a|enter loop|show b|show e|e|d|show e|e|d|c|b|exit loop|a|result 22",
        "tails 0|show a|a|zero 0|result 0",
        "tails 1|show a|b|a|one 1|result 1",
        "tails 2|show a|show c|c|b|a|zero 2|result 2",
        "tails 3|show a|show c|c|b|a|one 3|result 3",
    ]
    .join("|")
        + "|";
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\n', "|"),
        expected
    );
}

// The listing patterns below (`j` branches, `(%rsp)` stores) are x86-64's;
// AArch64 listings have not been checked against them.
#[cfg(target_arch = "x86_64")]
/// Functions with a hot loop and exits of each kind: a lone `?` failure, tail
/// calls and returns, each carrying the loop's total and releasing an owned
/// list.
const HOT_EXITS: &str = r#"def flips(xs: &mut list[i64]) -> Result[i64, OverflowError[i64]]:
    mut count = 0
    mut first = xs[0]
    while first != 0:
        mut lo = 0
        mut hi = first
        while lo < hi:
            xs.swap(lo, hi)
            lo += 1
            hi -= 1
        count += 1
        first = xs[0]
    Ok(count)

def one_failure(n: i64) -> Result[i64, AllocError | OverflowError[i64]]:
    mut xs = [3, 1, 2, 0]?
    mut total = 0
    for k in range(n):
        xs[0] = 3
        total += flips(&mut xs)?
    Ok(total)

def other(c: i64, n: i64) -> Result[i64, AllocError | OverflowError[i64]]:
    Ok(c + n)

def tail(n: i64) -> Result[i64, AllocError | OverflowError[i64]]:
    mut xs = [3, 1, 2, 0]?
    if n < 0:
        return other(0, n)
    mut total = 0
    for k in range(n):
        xs[0] = 3
        total += flips(&mut xs)?
        if total > 1000000000:
            return other(total, k)
    other(total, n)

def returns(n: i64) -> Result[i64, AllocError | OverflowError[i64]]:
    mut xs = [3, 1, 2, 0]?
    if n < 0:
        return Ok(-1)
    mut total = 0
    for k in range(n):
        xs[0] = 3
        total += flips(&mut xs)?
        if total > 1000000000:
            return Ok(total)
    Ok(total)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    print(one_failure(10)?, tail(10)?, returns(10)?)?
    Ok(())
"#;

#[cfg(target_arch = "x86_64")]
/// The instructions of each innermost loop's hot path in a `--dump asm`
/// listing: the blocks between a backward jump and its target from which the
/// jump is reachable, so exits and cold blocks are left out.
fn hot_loops(body: &str) -> Vec<Vec<&str>> {
    let target = |text: &str| -> Option<u64> {
        let operand = text.split_whitespace().nth(1)?;
        u64::from_str_radix(operand.strip_prefix("0x")?, 16).ok()
    };
    let code: Vec<(u64, &str)> = body
        .lines()
        .filter_map(|line| {
            let (addr, text) = line.trim().split_once(':')?;
            Some((u64::from_str_radix(addr, 16).ok()?, text.trim()))
        })
        .collect();
    let jumps = |text: &str| text.starts_with('j');
    let ends = |text: &str| text.starts_with("jmp") || text.starts_with("ret") || text == "ud2";
    let dests: Vec<u64> = code
        .iter()
        .filter(|(_, text)| jumps(text))
        .filter_map(|(_, text)| target(text))
        .collect();
    let mut blocks: Vec<Vec<(u64, &str)>> = vec![Vec::new()];
    for &(addr, text) in &code {
        if dests.contains(&addr) && !blocks.last().unwrap().is_empty() {
            blocks.push(Vec::new());
        }
        blocks.last_mut().unwrap().push((addr, text));
        if jumps(text) || ends(text) {
            blocks.push(Vec::new());
        }
    }
    blocks.retain(|block| !block.is_empty());
    let start = |addr: u64| blocks.iter().position(|block| block[0].0 == addr);
    let successors = |i: usize| {
        let (_, text) = *blocks[i].last().unwrap();
        let mut out: Vec<usize> = jumps(text)
            .then(|| target(text).and_then(start))
            .flatten()
            .into_iter()
            .collect();
        if !ends(text) && i + 1 < blocks.len() {
            out.push(i + 1);
        }
        out
    };
    let ranges: Vec<(usize, usize)> = (0..blocks.len())
        .filter_map(|i| {
            let (addr, text) = *blocks[i].last().unwrap();
            let dest = target(text).filter(|&dest| jumps(text) && dest <= addr)?;
            Some((start(dest)?, i))
        })
        .collect();
    ranges
        .iter()
        .filter(|&&(head, tail)| {
            !ranges
                .iter()
                .any(|&other| other != (head, tail) && head <= other.0 && other.1 <= tail)
        })
        .filter_map(|&(head, tail)| {
            let mut body = vec![tail];
            let mut changed = true;
            while changed {
                changed = false;
                for i in head..=tail {
                    if !body.contains(&i) && successors(i).iter().any(|s| body.contains(s)) {
                        body.push(i);
                        changed = true;
                    }
                }
            }
            // A backward jump in cold code whose target cannot reach it
            // closes no loop.
            body.contains(&head).then(|| {
                body.sort();
                body.iter()
                    .flat_map(|&i| blocks[i].iter().map(|&(_, text)| text))
                    .collect()
            })
        })
        .collect()
}

/// Values passed to a function's shared exit blocks stay out of its hot
/// loops (issue #811). Passed as block parameters, a lone failure's error,
/// tail calls' arguments and returned values joined the loop's total in one
/// register-allocation bundle spanning the function, and splitting it stored
/// and reloaded the total on every iteration.
#[cfg(target_arch = "x86_64")]
#[test]
fn values_passed_to_shared_exits_stay_out_of_hot_loops() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), HOT_EXITS).unwrap();
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args(["--dump", "asm", "p.plenty", "-o", "out"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = std::fs::read_to_string(dir.path().join("out/asm.txt")).unwrap();
    for name in ["one_failure", "tail", "returns"] {
        let body = support::asm_body(&asm, name);
        let loops = hot_loops(body);
        assert!(!loops.is_empty(), "no loop in {name}\n{body}");
        for hot in loops {
            // Loop-invariant loads may stay; a store is a spill.
            let stores: Vec<&&str> = hot
                .iter()
                .filter(|text| {
                    text.split('#')
                        .next()
                        .unwrap()
                        .trim_end()
                        .ends_with("(%rsp)")
                })
                .collect();
            assert!(
                stores.is_empty(),
                "{name}'s hot loop stores {stores:?}\n{body}"
            );
        }
    }
}

/// A loop inlined through a loop-free callee marks the outer caller too, so
/// `driver`'s exits after the call stage their values (issue #811). The
/// reloads address the staging slot at offset 8.
#[test]
fn loops_inlined_through_a_callee_stage_the_callers_exits() {
    let program = r#"def work(xs: &list[i64], n: i64) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    for k in range(n):
        total += xs[k & 3]
    Ok(total)

def mid(xs: &list[i64], n: i64) -> Result[i64, OverflowError[i64]]:
    t = work(xs, n)?
    Ok(t + 1)

def driver(n: i64) -> Result[i64, AllocError | OverflowError[i64]]:
    ys = [1, 2, 3, 4]?
    if n < 0:
        return Ok(-1)
    t = mid(&ys, n)?
    if t > 5:
        return Ok(t + ys[0])
    Ok(t)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    print(driver(100)?)?
    Ok(())
"#;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), program).unwrap();
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir.path())
        .env("PLENTY_CACHE", "off")
        .args([
            "--dump",
            "clif",
            "--dump-filter",
            "driver",
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
    let clif = std::fs::read_to_string(dir.path().join("out/clif.txt")).unwrap();
    // Both callees must be inlined for this to test the propagation.
    assert!(
        !clif.contains("__plenty_fn_work") && !clif.contains("__plenty_fn_mid"),
        "{clif}"
    );
    assert!(
        clif.lines()
            .any(|line| line.contains("stack_addr") && line.trim_end().ends_with("+8")),
        "driver's exits pass parameters\n{clif}"
    );
}
