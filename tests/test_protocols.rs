mod support;

#[test]
fn structural_methods_work_with_shared_mutable_and_returned_borrows() {
    let out = support::run(
        r#"
protocol Readable:
    def read(self) -> str:
        pass
protocol Incrementable:
    def increment(self: &mut Incrementable, amount: i64) -> ():
        pass
    def view(self) -> &i64:
        pass
class Text:
    value: str
    def read(self) -> str:
        self.value
class Counter:
    value: i64
    def increment(self: &mut Counter, amount: i64) -> ():
        self.value = self.value + amount
    def view(self) -> &i64:
        &self.value
def read[T: Readable](source: &T) -> str:
    source.read()
def increment[T: Incrementable](source: &mut T) -> ():
    source.increment(2)
    print(source.view()).unwrap()
text = Text("hello")
print(read[Text](&text)).unwrap()
print(read(&text)).unwrap()
mut counter = Counter(3)
increment[Counter](&mut counter)
increment(&mut counter)
print(counter.value).unwrap()
"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "hello\nhello\n5\n7\n7\n"
    );
}

#[test]
fn constraints_check_every_required_method_and_exact_signature() {
    let prefix = "protocol Readable:\n    def read(self) -> i64:\n        pass\ndef accepts[T: Readable](value: &T) -> ():\n    pass\n";
    for (declaration, expected) in [
        ("class Bad:\n    value: i64\n", "missing method `read`"),
        ("class Bad:\n    value: i64\n    def read(self) -> str:\n        return 'bad'\n", "signature of `read`"),
        ("class Bad:\n    value: i64\n    def read(self: &mut Bad) -> i64:\n        self.value\n", "signature of `read`"),
        ("class Bad:\n    value: i64\n    def read(self, n: i64) -> i64:\n        n\n", "signature of `read`"),
    ] {
        for callee in ["accepts[Bad]", "accepts"] {
            let source = format!("{prefix}{declaration}x = Bad(1)\n{callee}(&x)");
            let error = support::check_source(&source).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
    }
    for source in [
        "protocol P:\n    def f(self) -> i64:\n        1",
        "protocol P:\n    def f(self: i64) -> i64:\n        pass",
        "protocol P:\n    def f(self, x: ()) -> ():\n        pass",
        "protocol P:\n    def f(self) -> Missing:\n        pass",
        "protocol P:\n    def f(self) -> i64:\n        pass\ndef f(x: P) -> ():\n    pass",
    ] {
        assert!(support::check_source(source).is_err(), "{source}");
    }
}
