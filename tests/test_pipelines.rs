mod support;

fn output(source: &str, expected: &str) {
    let result = support::run(source);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout), expected);
}

#[test]
fn fused_range_and_borrowed_list_fold() {
    output(
        r#"
def even(n: &i64) -> bool:
    *n % 2 == 0
def square(n: i64) -> i64:
    n * n
def add(a: i64, b: i64) -> i64:
    a + b
def main() -> ():
    p = iter(range(10)).filter(even).map(square)
    print(p.fold(0, add)).unwrap()
    values = [1, 2, 3, 4].unwrap()
    total = iter(&values).map(def(n: &i64) -> i64: *n * 2).fold(0, add)
    print(total, len(values)).unwrap()
"#,
        "120\n20 4\n",
    );
}

#[test]
fn adapters_preserve_order_and_bounds() {
    output(
        r#"
def main() -> ():
    for n in iter(range(10)).skip(2).take(4).filter(def(n: &i64) -> bool: *n % 2 == 1):
        print(n).unwrap()
    print(iter(range(10)).take_while(def(n: &i64) -> bool: *n < 3).count()).unwrap()
    print(iter(range(10)).take(0).count(), iter(range(10)).skip(-1).count()).unwrap()
"#,
        "3\n5\n3\n0 10\n",
    );
}

#[test]
fn pipeline_is_affine() {
    let error = support::check_source(
        r#"
def main() -> ():
    p = iter(range(3))
    print(p.count()).unwrap()
    print(p.count()).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("moved"), "{error}");
}

#[test]
fn source_loan_protects_delayed_consumption() {
    let error = support::check_source(
        r#"
def main() -> ():
    mut values = [1, 2].unwrap()
    p = iter(&values)
    values.append(3).unwrap()
    print(p.count()).unwrap()
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("borrow"), "{error}");
}

#[test]
fn callbacks_and_owned_sources_are_released_on_break_and_drop() {
    output(
        r#"
class Guard:
    n: i64
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def main() -> ():
    values = [Guard(1), Guard(2), Guard(3)].unwrap()
    capture = Guard(9)
    p = iter(values).map(def [capture](g: Guard) -> i64: g.n + capture.n)
    for n in p:
        print(n).unwrap()
        break
    print("after").unwrap()
    unused = iter([Guard(4)].unwrap()).map(def(g: Guard) -> i64: g.n)
    drop(unused)
    print("done").unwrap()
"#,
        "drop 1\n10\ndrop 9\ndrop 2\ndrop 3\nafter\ndrop 4\ndone\n",
    );
}

#[test]
fn terminal_short_circuit_and_empty_reduction() {
    output(
        r#"
def add(a: i64, b: i64) -> i64:
    a + b
def main() -> ():
    print(iter(range(5)).reduce(add), iter(range(0)).reduce(add)).unwrap()
    print(iter(range(5)).any(def(n: &i64) -> bool: *n == 2)).unwrap()
    print(iter(range(5)).all(def(n: &i64) -> bool: *n < 3)).unwrap()
    print(iter(range(5)).find(def(n: &i64) -> bool: *n > 2)).unwrap()
    print(iter(range(0)).any(def(n: &i64) -> bool: True), iter(range(0)).all(def(n: &i64) -> bool: False)).unwrap()
"#,
        "Option[i64].Some(10) Option[i64].Nothing\nTrue\nFalse\nOption[i64].Some(3)\nFalse True\n",
    );
}

#[test]
fn captured_mutable_borrow_is_retained_until_consumption() {
    output(
        r#"
def main() -> ():
    mut seen = 0
    visit = def [&mut seen](n: i64) -> i64:
        seen += 1
        n * 2
    p = iter(range(5)).map(visit)
    print(p.take(2).count(), seen).unwrap()
"#,
        "2 2\n",
    );
}

#[test]
fn local_type_annotation_and_name_shadowing() {
    output(
        r#"
def main() -> ():
    p: Iter[i64] = iter(range(3))
    print(p.count()).unwrap()
"#,
        "3\n",
    );
    output(
        r#"
def iter(n: i64) -> i64:
    n + 1
def main() -> ():
    print(iter(3)).unwrap()
"#,
        "4\n",
    );
}

#[test]
fn unsupported_placements_are_rejected() {
    for source in [
        "def bad() -> Iter[i64]:\n    iter(range(3))\n",
        "def bad(p: Iter[i64]) -> ():\n    pass\n",
        "def bad(p: &mut Iter[i64]) -> ():\n    pass\n",
        "def bad() -> Option[Iter[i64]]:\n    Nothing\n",
        "p = iter(range(3))\nprint(&p).unwrap()\n",
        "p = iter(range(3))\nprint(p).unwrap()\n",
        "p = iter(range(3))\nf = (def [p]() -> i64: p.count())\n",
        "p = iter(range(3))\nvalues = [p].unwrap()\n",
        "def bad() -> Generator[i64]:\n    p = iter(range(3))\n    yield 1\n",
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains("pipeline"), "{source}\n{error}");
    }
}

#[test]
fn empty_and_dropped_pipeline_does_not_call_callbacks_or_advance_generator() {
    output(
        r#"
def numbers() -> Generator[i64]:
    print("advanced").unwrap()
    yield 1
def visit(n: i64) -> i64:
    print("callback").unwrap()
    n
def main() -> ():
    p = iter(numbers()).map(visit)
    drop(p)
    print(iter(numbers()).map(visit).take(0).count()).unwrap()
"#,
        "0\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn non_collecting_terminals_do_not_allocate() {
    output(
        r#"
def add(a: i64, b: i64) -> i64:
    a + b
def visit(n: i64) -> ():
    pass
def main() -> ():
    values = [1, 2, 3].unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    print("__test_begin_no_allocations__").unwrap()
    a = iter(&values).map(def(n: &i64) -> i64: *n).fold(0, add)
    b = iter(range(3)).reduce(add)
    c = iter(range(3)).count()
    d = iter(range(3)).any(def(n: &i64) -> bool: *n == 1)
    e = iter(range(3)).all(def(n: &i64) -> bool: *n < 3)
    f = iter(range(3)).find(def(n: &i64) -> bool: *n == 2)
    iter(range(3)).for_each(visit)
    print("__test_end_no_allocations__").unwrap()
    print("__test_restore_allocations__").unwrap()
    print(a, b, c, d, e, f).unwrap()
"#,
        "__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\n6 Option[i64].Some(3) 3 True True Option[i64].Some(2)\n",
    );
}

#[test]
fn fused_range_and_borrowed_list_have_no_adapter_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("pipeline.plenty");
    std::fs::write(&source, r#"
def add(a: i64, b: i64) -> i64:
    a + b
def range_total(n: i64) -> i64:
    iter(range(n)).filter(def(n: &i64) -> bool: *n % 2 == 0).map(def(n: i64) -> i64: n * n).fold(0, add)
def borrowed_total(values: &list[i64]) -> i64:
    iter(values).map(def(n: &i64) -> i64: *n).fold(0, add)
def main() -> ():
    values = [1, 2, 3].unwrap()
    print(range_total(10), borrowed_total(&values)).unwrap()
"#).unwrap();
    for function in ["range_total", "borrowed_total"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_plenty"))
            .args(["--release", "--dump", "clif,asm", "--dump-filter", function])
            .arg(&source)
            .args(["-o", "-"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let dump = String::from_utf8(output.stdout).unwrap();
        for absent in [
            "call_indirect",
            "plenty_collection_call",
            "plenty_generator_resume",
        ] {
            assert!(!dump.contains(absent), "{function}: {absent}\n{dump}");
        }
        assert!(dump.contains("call"), "{dump}");
    }
}
