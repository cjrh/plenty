mod support;

fn run(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let visible = text
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim_end(), "{source}");
}

fn reject(source: &str, expected: &str) {
    let error = support::check_source(source).unwrap_err().to_string();
    assert!(
        error.contains(expected),
        "{source}\nexpected {expected}, got {error}"
    );
}

#[test]
fn shared_calls_accept_bindings_literals_explicit_borrows_and_forwarded_references() {
    run(
        r#"
def size(s: &str) -> i64:
    len(s)
def view(s: &str) -> &str:
    s
def byte(n: &u8) -> u8:
    *n
def forward(s: &mut str) -> i64:
    size(s)
def generic[T](value: &T) -> ():
    print(value).unwrap()
mut s = "ada"
r = &s
print(size(s)).unwrap()
print(size(&s)).unwrap()
print(size(r)).unwrap()
print(size("ada")).unwrap()
print(size(&"ada")).unwrap()
print(size(view(s))).unwrap()
print(size(view("ada"))).unwrap()
print(forward(&mut s)).unwrap()
print(byte(5)).unwrap()
print(byte(&5)).unwrap()
generic("ada")
generic[u8](7)
print(contains(&"ada", &"d")).unwrap()
print(len(&"ada")).unwrap()
print(s).unwrap()
"#,
        "3\n3\n3\n3\n3\n3\n3\n3\n5\n5\nada\n7\nTrue\n3\nada\n",
    );
}

#[test]
fn owned_temporary_is_evaluated_once_and_lives_through_the_full_expression() {
    run(
        r#"
class Item:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
def make() -> Item:
    print("make").unwrap()
    Item(42)
def later() -> i64:
    print("later").unwrap()
    1
def view(item: &Item) -> &i64:
    &item.value
def use(item: &Item, extra: i64) -> i64:
    item.value + extra
print(use(make(), later())).unwrap()
print(*view(make()) + later()).unwrap()
print("end").unwrap()
"#,
        "make\nlater\n43\n42\nmake\nlater\n43\n42\nend\n",
    );
}

#[test]
fn later_argument_error_cleans_the_borrowed_temporary() {
    run(
        r#"
class Item:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
def fail() -> Result[i64, str]:
    Err("stop")
def use(item: &Item, extra: i64) -> i64:
    item.value + extra
def attempt() -> Result[i64, str]:
    Ok(use(Item(42), fail()?))
drop(attempt())
print("done").unwrap()
"#,
        "42\ndone\n",
    );
}

#[test]
#[cfg(feature = "runtime-checks")]
fn borrowing_itself_does_not_allocate() {
    run(
        r#"
def size(s: &str) -> i64:
    len(s)
def first(xs: &list[i64]) -> i64:
    xs[0]
xs = [42].unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
a = size("short")
b = first(xs)
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(a).unwrap()
print(b).unwrap()
"#,
        "5\n42\n",
    );
}

#[test]
fn later_arguments_cannot_move_or_mutate_an_implicitly_borrowed_owner() {
    reject(
        "def f(a: &list[i64], b: list[i64]) -> ():\n    pass\nxs = [1].unwrap()\nf(xs, xs)\n",
        "conflicting borrow",
    );
    reject("def f(a: &list[i64], b: &mut list[i64]) -> ():\n    pass\nmut xs = [1].unwrap()\nf(xs, &mut xs)\n", "conflicting borrow");
    reject(
        "def f(a: &mut i64) -> ():\n    pass\nmut n = 1\nf(n)\n",
        "explicit &mut",
    );
    reject(
        "def f(a: &mut i64) -> ():\n    pass\nf(&mut 1)\n",
        "named binding",
    );
    reject(
        "def f(a: list[i64]) -> ():\n    pass\nxs = [1].unwrap()\nf(xs)\nprint(xs).unwrap()\n",
        "moved binding",
    );
}

#[test]
fn numeric_context_preserves_typed_variable_and_range_errors() {
    for literal in ["1.0", "-1.0", "1e2"] {
        reject(
            &format!("def f(n: u64) -> ():\n    pass\nf({literal})\n"),
            "expected u64, got f64",
        );
        reject(
            &format!("f = open(\"unused\").unwrap()\nf.seek({literal}).unwrap()\n"),
            "expected u64, got f64",
        );
    }
    reject(
        "def f(n: &u8) -> ():\n    pass\nn = 5\nf(n)\n",
        "expected &u8, got &i64",
    );
    reject("def f(n: &u8) -> ():\n    pass\nf(256)\n", "out of range");
    reject("def f(n: &u8) -> ():\n    pass\nf(-1)\n", "out of range");
    run("xs = [1u8, 2u8].unwrap()\nprint(xs.count(1)).unwrap()\nprint(xs.find(&2)).unwrap()\nd = {1u8: 9}.unwrap()\nprint(d.get(1)).unwrap()\n", "1\nOption[i64].Some(1)\nOption[i64].Some(9)\n");
}

#[test]
fn borrowed_temporary_arguments_keep_the_caller_alive_at_tail_positions() {
    run(
        r#"
class Item:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
def read(item: &Item) -> i64:
    print("read").unwrap()
    item.value
def explicit() -> i64:
    return read(Item(11))
def implicit() -> i64:
    read(Item(12))
def text(s: &str) -> i64:
    len(s)
def scalar() -> i64:
    text("a temporary string")
print(explicit()).unwrap()
print(implicit()).unwrap()
print(scalar()).unwrap()
"#,
        "read\n11\n11\nread\n12\n12\n18\n",
    );
}

#[test]
fn callbacks_and_methods_use_the_same_signature_directed_borrows() {
    run(
        r#"
def size(s: &str) -> i64:
    len(s)
def invoke(f: &Closure[[], i64]) -> i64:
    f()
class Reader:
    def size(self, s: &str) -> i64:
        len(s)
reader = Reader()
f = size
print(f("ada")).unwrap()
print(reader.size("ada")).unwrap()
count = 7
read = def [&count]() -> i64:
    count
print(invoke(read)).unwrap()
"#,
        "3\n3\n7\n",
    );
}

#[test]
fn a_temporary_callback_keeps_its_captures_borrowed_across_later_arguments() {
    reject(
        r#"
def invoke(f: &Closure[[], i64], ignored: i64) -> i64:
    f()
def change(values: &mut list[i64]) -> i64:
    values[0] = 9
    0
mut values = [7].unwrap()
read = def [&values]() -> i64:
    values[0]
print(invoke(read if True else read, change(&mut values))).unwrap()
"#,
        "conflicting borrow",
    );
}
