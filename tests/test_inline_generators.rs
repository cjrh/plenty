mod support;

fn expect(source: &str, expected: &str) {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn suspended_frames_move_through_factories_and_consumers() {
    expect(
        r#"
def numbers(n: i64) -> Generator[i64]:
    yield n
    yield n + 1
def make(n: i64) -> Generator[i64]:
    numbers(n)
def forward(g: Generator[i64]) -> Generator[i64]:
    g
def take(g: &mut Generator[i64]) -> Option[i64]:
    next(g)
mut original = make(7)
print(next(original)).unwrap()
mut moved = forward(original)
print(take(&mut moved)).unwrap()
print(next(moved)).unwrap()
print(next(moved)).unwrap()
"#,
        "Option[i64].Some(7)\nOption[i64].Some(8)\nOption[i64].Nothing\nOption[i64].Nothing\n",
    );
}

#[test]
fn nested_generators_survive_moving_the_suspended_parent() {
    expect(
        r#"
def numbers() -> Generator[i64]:
    yield 3
    yield 6
def wrapped(source: Generator[i64]) -> Generator[i64]:
    for value in source:
        yield value
    yield 9
mut a = wrapped(numbers())
print(next(a)).unwrap()
mut b = a
print(next(b)).unwrap()
print(next(b)).unwrap()
print(next(b)).unwrap()
"#,
        "Option[i64].Some(3)\nOption[i64].Some(6)\nOption[i64].Some(9)\nOption[i64].Nothing\n",
    );
}

#[test]
fn factories_preserve_concrete_frames_in_option_and_result() {
    expect(
        r#"
def numbers(n: i64) -> Generator[i64]:
    yield n
def optional(n: i64) -> Option[Generator[i64]]:
    if n < 0:
        return Nothing
    Some(numbers(n))
def checked(n: i64) -> Result[Generator[i64], i64]:
    if n < 0:
        return Err(n)
    Ok(numbers(n))
def forward(n: i64) -> Result[Generator[i64], i64]:
    source = checked(n)?
    Ok(source)
def use(source: Option[Generator[i64]]) -> i64:
    match source:
        case Some(values):
            for n in values:
                return n
            return 0
        case Nothing:
            return -1
print(use(optional(12))).unwrap()
print(use(optional(-1))).unwrap()
mut source = forward(14).unwrap()
print(next(source)).unwrap()
"#,
        "12\n-1\nOption[i64].Some(14)\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn scalar_frames_work_with_all_heap_allocation_disabled() {
    expect(r#"
def numbers(n: i64) -> Generator[i64]:
    for value in range(n):
        yield value
def outer(source: Generator[i64]) -> Generator[i64]:
    for value in source:
        yield value + 1
def make() -> Result[Generator[i64], AllocError]:
    Ok(outer(numbers(4)))
def forward(source: Option[Generator[i64]]) -> Option[Generator[i64]]:
    source
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
mut source = forward(Some(make().unwrap())).unwrap()
first = next(source).unwrap()
mut moved = source
mut total = first
for value in moved:
    total = total + value
drop(make())
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
print(total).unwrap()
"#, "__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n10\n");
}

#[test]
fn consumers_specialize_by_producer_and_preserve_generic_arguments() {
    expect(
        r#"
type Source = Generator[i64]
def once[T](value: T) -> Generator[T]:
    yield value
def pair() -> Generator[i64]:
    yield 10
    yield 20
def forward(source: Source) -> Source:
    source
def total(source: Source) -> i64:
    mut sum = 0
    for value in source:
        sum = sum + value
    sum
def generic_total[T: IntType](source: Generator[T], start: T) -> T:
    mut sum = start
    for value in source:
        sum = sum + value
    sum
print(total(forward(once(7)))).unwrap()
print(total(forward(pair()))).unwrap()
print(generic_total(once(5u8), 2u8)).unwrap()
print(generic_total[u8](once[u8](6), 2u8)).unwrap()
"#,
        "7\n30\n7\n8\n",
    );
}

#[test]
fn captures_drop_once_before_resume_after_suspend_and_on_exhaustion() {
    expect(
        r#"
class Resource:
    n: i64
    def __del__(self: &mut Resource) -> ():
        print(self.n).unwrap()
def values(resource: Resource) -> Generator[i64]:
    local = Resource(resource.n + 10)
    yield resource.n
    yield local.n
drop(values(Resource(1)))
mut a = values(Resource(2))
print(next(a)).unwrap()
b = a
drop(Some(b))
mut c = values(Resource(3))
for n in c:
    print(n).unwrap()
mut d = values(Resource(4))
for n in d:
    break
"#,
        "1\nOption[i64].Some(2)\n12\n2\n3\n13\n13\n3\n14\n4\n",
    );
}

#[test]
fn generator_created_inside_a_drop_hook_finishes_before_the_hook_continues() {
    expect(
        r#"
class Resource:
    n: i64
    def __del__(self: &mut Resource) -> ():
        print(self.n).unwrap()
def pending(resource: Resource) -> Generator[i64]:
    print("unexpected resume").unwrap()
    yield resource.n
def discard(source: Generator[i64]) -> ():
    drop(source)
class Owner:
    n: i64
    def __del__(self: &mut Owner) -> ():
        discard(pending(Resource(self.n)))
        print("finished").unwrap()
drop(Owner(7))
"#,
        "7\nfinished\n",
    );
}

#[test]
fn incompatible_producers_and_recursive_inline_layouts_have_diagnostics() {
    let cases = [
        (
            "drop(Option[Generator[i64]].Nothing)",
            "cannot infer a concrete generator type",
        ),
        (
            r#"
def a() -> Generator[i64]:
    for n in b():
        yield n
def b() -> Generator[i64]:
    for n in a():
        yield n
drop(a())
"#,
            "recursive inline generator layout",
        ),
        (
            r#"
def a() -> Generator[i64]:
    yield 1
def b() -> Generator[i64]:
    yield 2
def choose(flag: bool) -> Generator[i64]:
    if flag:
        return a()
    b()
drop(choose(True))
"#,
            "expected Generator[i64] from `a`, got Generator[i64] from `b`",
        ),
        (
            r#"
def nested() -> Generator[i64]:
    for n in nested():
        yield n
drop(nested())
"#,
            "recursive inline generator layout",
        ),
        (
            r#"
def factory() -> Generator[i64]:
    factory()
drop(factory())
"#,
            "recursive generator factory",
        ),
        (
            r#"
def values() -> Generator[i64]:
    yield 1
source = values()
drop(source)
drop(source)
"#,
            "moved",
        ),
    ];
    for (source, expected) in cases {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}; expected {expected}");
    }
}

#[test]
fn nested_sum_returns_and_generator_error_payloads_keep_each_frame_shape() {
    expect(
        r#"
def short() -> Generator[i64]:
    yield 1
def long() -> Generator[i64]:
    for n in range(3):
        yield n
def nested(flag: bool) -> Result[Option[Generator[i64]], i64]:
    if flag:
        return Ok(Nothing)
    Ok(Some(long()))
def alternative(flag: bool) -> Result[Generator[i64], Generator[i64]]:
    if flag:
        return Err(long())
    Ok(short())
def total(source: Generator[i64]) -> i64:
    mut n = 0
    for value in source:
        n = n + value
    n
def use_nested(flag: bool) -> i64:
    match nested(flag).unwrap():
        case Some(source):
            return total(source)
        case Nothing:
            return -1
def use_alternative(flag: bool) -> i64:
    match alternative(flag):
        case Ok(source):
            return total(source)
        case Err(source):
            return total(source)
print(use_nested(True)).unwrap()
print(use_nested(False)).unwrap()
print(use_alternative(True)).unwrap()
print(use_alternative(False)).unwrap()
"#,
        "-1\n3\n3\n1\n",
    );
}

#[test]
fn range_and_frame_alternatives_survive_calls_and_drop() {
    expect(
        r#"
def values() -> Generator[i64]:
    for n in range(3):
        yield n
def either(flag: bool) -> Result[range[i64], Generator[i64]]:
    if flag:
        return Ok(range(4))
    Err(values())
def consume(flag: bool) -> i64:
    match either(flag):
        case Ok(r):
            return len(r)
        case Err(g):
            mut total = 0
            for n in g:
                total = total + n
            return total
print(consume(True)).unwrap()
print(consume(False)).unwrap()
drop(either(False))
"#,
        "4\n3\n",
    );
}

#[test]
fn return_and_propagation_drop_suspended_captures_and_pending_arguments() {
    expect(
        r#"
class Resource:
    n: i64
    def __del__(self: &mut Resource) -> ():
        print(self.n).unwrap()
def values(resource: Resource) -> Generator[i64]:
    yield resource.n
    print("unexpected resume").unwrap()
def fail() -> Result[i64, i64]:
    Err(9)
def receive(source: Generator[i64], value: i64) -> i64:
    value
def run() -> Result[i64, i64]:
    mut source = values(Resource(1))
    first = next(source).unwrap()
    receive(values(Resource(2)), fail()?)
    Ok(first)
print(run()).unwrap()
"#,
        "2\n1\nResult[i64, i64].Err(9)\n",
    );
}

#[test]
fn generator_construction_snapshots_moves_before_later_arguments_mutate_locals() {
    expect(
        r#"
def values(start: i64) -> Generator[i64]:
    yield start
    yield start + 1
def advance(source: &mut Generator[i64]) -> i64:
    next(source).unwrap()
def choose(a: Generator[i64], b: Generator[i64], n: i64) -> Generator[i64]:
    drop(b)
    a
mut a = values(1)
mut b = values(10)
first = advance(&mut a)
mut result = choose(a, values(20), advance(&mut b))
print(first).unwrap()
print(next(result)).unwrap()
print(next(b)).unwrap()
"#,
        "1\nOption[i64].Some(2)\nOption[i64].Some(11)\n",
    );
}
