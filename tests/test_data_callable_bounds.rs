mod support;

#[test]
fn generic_data_infers_callback_signature_parameters_and_checks_call_mode() {
    let output = support::run(
        r#"
class Handler[T, F: Callable[[T], T]]:
    callback: F
    def call(self: &mut Handler[T, F], value: T) -> T:
        self.callback(value)
enum Job[T, F: OnceCallable[[], T]]:
    Run(F)
def named(n: u8) -> u8:
    n + 1
def package[T, F: OnceCallable[[], T]](callback: F) -> Result[Job[T, F], AllocError]:
    Job[T, F].Run(callback)
def consume[T, F: OnceCallable[[], T]](job: Job[T, F]) -> T:
    match job:
        case Job[T, F].Run(callback):
            mut owned = callback
            owned()
mut first = Handler(named).unwrap()
print(first.call(2)).unwrap()
offset = 10u8
callback = def [offset](n: u8) -> u8:
    offset + n
mut second = Handler(callback).unwrap()
print(second.call(3)).unwrap()
values = [42].unwrap()
job = def once [values]() -> list[i64]:
    values
print(consume(package(job).unwrap())).unwrap()
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"3\n13\n[42]\n");
}

#[test]
fn constraints_are_checked_for_explicit_types_unused_declarations_and_call_modes() {
    for (source, expected) in [
        ("class Handler[F: Callable[[i64], i64]]:\n    callback: F\ntype Invalid = Handler[i64]\n", "does not satisfy Callable"),
        ("enum Job[F: OnceCallable[[], i64]]:\n    Run(F)\ntype Invalid = Job[str]\n", "does not satisfy OnceCallable"),
        ("class Bad[F: Callable[[Missing], i64]]:\n    callback: F\n", "Missing"),
        ("enum Bad[F: Callable[[], Missing]]:\n    Run(F)\n", "Missing"),
        ("class Bad[F: Callable[[i64], i64]]:\n    callback: F\nf = def once() -> i64:\n    1\nx = Bad(f).unwrap()\n", "does not satisfy Callable"),
        ("class Bad[F: Callable[i64]]:\n    callback: F\n", "expected `[`"),
        ("class Bad[F: Callable[[Bad[F]], i64]]:\n    callback: F\ntype Invalid = Bad[i64]\n", "does not satisfy Callable"),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}

#[test]
fn callable_data_bounds_resolve_later_aliases_and_do_not_check_placeholder_instances() {
    support::check_source(
        r#"
type Concrete = Holder[Callable[[i64], Answer]]
type Answer = i64
class Holder[F: Callable[[i64], Answer]]:
    callback: F
protocol Source[F]:
    def get(self) -> Holder[F]:
        pass
def unused[F: Callable[[i64], Answer], G: Callable[[], Holder[F]]](callback: G) -> ():
    pass
"#,
    )
    .unwrap();
}
