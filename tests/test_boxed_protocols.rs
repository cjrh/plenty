mod support;
use support::assert_stdout as run;

fn check_error(source: &str, expected: &str) {
    let error = support::check_error(source);
    assert!(error.contains(expected), "expected {expected}: {error}");
}

const TYPES: &str = r#"
protocol Counter:
    def current(self) -> i64:
        pass
    def increment(self: &mut Counter) -> ():
        pass
class Count:
    value: i64
    def current(self) -> i64:
        self.value
    def increment(self: &mut Count) -> ():
        self.value += 1
enum Flag:
    Off
    On
    def current(self) -> i64:
        match self:
            case Flag.Off:
                0
            case Flag.On:
                1
    def increment(self: &mut Flag) -> ():
        *self = Flag.On
"#;

#[test]
fn heterogeneous_list_mutation_and_generics() {
    run(
        &format!(
            r#"{TYPES}
def read[T: Counter](value: &T) -> i64:
    value.current()
mut values: list[Box[Counter]] = [Box[Counter](Count(10)).unwrap(), Box[Counter](Flag.Off).unwrap()].unwrap()
for value in &mut values:
    value.increment()
    print(value.current(), read(value)).unwrap()
values.reverse()
value = values.pop().unwrap()
print(value.current()).unwrap()
"#
        ),
        "11 11\n1 1\n11\n",
    );
}

#[test]
fn reject_mutation_through_shared_owner() {
    check_error(
        &format!(
            r#"{TYPES}
value = Box[Counter](Count(0)).unwrap()
value.increment()
"#
        ),
        "mutable",
    );
}

#[test]
fn fixed_arguments_generic_payloads_and_temporaries() {
    run(
        r#"
protocol Readable[T]:
    def read(self) -> T:
        pass
class Cell[T]:
    value: T
    def read(self) -> T:
        self.value
def read[T, R: Readable[T]](source: &R) -> T:
    source.read()
def make(n: i64) -> Result[Box[Readable[i64]], AllocError]:
    Box[Readable[i64]](Cell(n))
type Reader = Box[Readable[i64]]
class Holder:
    reader: Reader
enum Job:
    Run(Reader)
mut readers: dict[str, Reader] = {"a": make(7).unwrap()}.unwrap()
print(read(&readers["a"])).unwrap()
holder = Holder(make(8).unwrap())
print(holder.reader.read()).unwrap()
job = Job.Run(make(9).unwrap())
match &job:
    case Job.Run(reader):
        print(reader.read()).unwrap()
print(make(10).unwrap().read()).unwrap()
"#,
        "7\n8\n9\n10\n",
    );
}

#[test]
fn owned_view_arguments_and_fallible_inline_results() {
    run(
        r#"
protocol Operations:
    def take(self, values: list[i64]) -> i64:
        pass
    def view(self, values: &[i64]) -> i64:
        pass
    def pair(self, n: i64) -> (i64, i64):
        pass
    def fallible(self, n: i64) -> Result[i64, AllocError]:
        pass
class Worker:
    def take(self, values: list[i64]) -> i64:
        values[0]
    def view(self, values: &[i64]) -> i64:
        values[0]
    def pair(self, n: i64) -> (i64, i64):
        (n, n + 1)
    def fallible(self, n: i64) -> Result[i64, AllocError]:
        Ok(n)
worker = Box[Operations](Worker()).unwrap()
values = [7, 8].unwrap()
print(worker.take([4].unwrap()), worker.view(&values[:]), worker.pair(10), worker.fallible(12).unwrap()).unwrap()
protocol Sized:
    def len(self) -> i64:
        pass
bytes_box = Box[Sized](b"abc").unwrap()
print(bytes_box.len()).unwrap()
"#,
        "4 7 (10, 11) 12\n3\n",
    );
}

#[test]
fn eligibility_checked_at_box_use() {
    for (method, expected) in [
        ("def get[T](self, value: T) -> T:", "method-local"),
        ("def get(self: P) -> i64:", "consuming receiver"),
        ("def get(self) -> &i64:", "returns a reference"),
        ("def get(self) -> Option[&i64]:", "returns a reference"),
        (
            "def get(self) -> Result[&i64, AllocError]:",
            "returns a reference",
        ),
        ("def get(self) -> &[i64]:", "returns a reference"),
        ("def get(self, other: &P) -> bool:", "outside its receiver"),
        ("def get(self) -> Box[P]:", "outside its receiver"),
    ] {
        check_error(
            &format!(
                "protocol P:\n    {method}\n        pass\ndef consume(value: Box[P]) -> ():\n    pass\n"
            ),
            expected,
        );
    }
}

#[test]
fn owners_keep_normal_restrictions() {
    for (tail, expected) in [
        ("moved = value\nprint(value.current()).unwrap()", "moved"),
        (
            "borrowed = &value\ndrop(value)\nprint(borrowed.current()).unwrap()",
            "borrow",
        ),
        ("copy(value)", "cannot be copied"),
        ("print(value == value).unwrap()", "equality"),
        ("print(value.extra()).unwrap()", "has no method"),
        ("Box[Counter](value)", "concrete owner directly"),
        ("plain = *value", "dereference"),
    ] {
        check_error(
            &format!("{TYPES}\nvalue = Box[Counter](Count(0)).unwrap()\n{tail}\n"),
            expected,
        );
    }
    check_error(
        &format!("{TYPES}\nvalues: list[Counter] = [].unwrap()\n"),
        "unknown type",
    );
    check_error(
        &format!("{TYPES}\nvalue: Box[Counter] = Box(Count(0)).unwrap()\n"),
        "expected Box[Counter]",
    );
}

#[test]
fn concrete_cleanup_runs_once_after_growth_replacement_and_extraction() {
    run(
        r#"
protocol Id:
    def id(self) -> i64:
        pass
class Guard:
    n: i64
    def id(self) -> i64:
        self.n
    def __del__(self) -> ():
        print("drop", self.n).unwrap()
def make(n: i64) -> Result[Box[Id], AllocError]:
    Box[Id](Guard(n))
mut values: list[Box[Id]] = [].unwrap()
for n in range(5):
    values.append(make(n).unwrap()).unwrap()
values.reverse()
first = values.pop(0).unwrap()
print(first.id()).unwrap()
drop(first)
for item in values:
    print(item.id()).unwrap()
    break
mut entries = {"a": make(10).unwrap()}.unwrap()
entries.insert("a", make(11).unwrap()).unwrap()
value = entries.pop("a").unwrap()
drop(entries)
drop(value)
"#,
        "4\ndrop 4\n3\ndrop 3\ndrop 2\ndrop 1\ndrop 0\ndrop 10\ndrop 11\n",
    );
}

#[test]
fn nested_worker_and_shared_boundaries_reject_erasure() {
    for ty in ["Box[Counter]", "list[Box[Counter]]", "Holder"] {
        check_error(
            &format!(
                "{TYPES}\nclass Holder:\n    value: Box[Counter]\nsender, receiver = channel[{ty}](1).unwrap()\n"
            ),
            "not certified for worker threads",
        );
    }
    check_error(
        &format!("{TYPES}\nvalue = Shared(Box[Counter](Count(0)).unwrap()).unwrap()\n"),
        "erased payloads",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failure_and_allocation_free_calls_drop_exactly_once() {
    run(
        r#"
protocol Id:
    def id(self) -> i64:
        pass
class Guard:
    n: i64
    def id(self) -> i64:
        self.n
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
held = Box[Id](Guard(7)).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
match Box[Id](Guard(8)):
    case Ok(value):
        drop(value)
    case Err(_):
        write_stdout("failed\n").unwrap()
        pass
n = held.id()
drop(held)
print("__test_restore_allocations__").unwrap()
print(n).unwrap()
"#,
        "__test_fail_allocations_after_0__\ndrop\nfailed\ndrop\n__test_restore_allocations__\n7\n",
    );
}

#[test]
fn mutable_dictionary_projection_and_receiver_reservation() {
    run(
        &format!(
            r#"{TYPES}
mut values = {{"a": Box[Counter](Count(1)).unwrap()}}.unwrap()
match values.get_mut("a"):
    case Some(value):
        value.increment()
        print(value.current()).unwrap()
    case Nothing:
        pass
"#
        ),
        "2\n",
    );
    check_error(
        &format!(
            r#"{TYPES}
mut values = [Box[Counter](Count(1)).unwrap()].unwrap()
borrowed = &values[0]
values.clear()
print(borrowed.current()).unwrap()
"#
        ),
        "borrow",
    );
}

#[test]
fn explicit_box_pattern_infers_fixed_argument() {
    run(
        r#"
protocol Readable[T]:
    def read(self) -> T:
        pass
class Cell:
    n: i64
    def read(self) -> i64:
        self.n
def read[T](value: &Box[Readable[T]]) -> T:
    value.read()
value = Box[Readable[i64]](Cell(12)).unwrap()
print(read(&value)).unwrap()
"#,
        "12\n",
    );
}

#[test]
fn public_protocol_can_hide_private_implementation() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("api.plenty"),
        r#"
pub protocol Reader:
    def read(self) -> i64:
        pass
class Hidden:
    n: i64
    pub def read(self) -> i64:
        self.n
pub def make() -> Result[Box[Reader], AllocError]:
    Box[Reader](Hidden(12))
"#,
    )
    .unwrap();
    let main = directory.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
from api import Reader as View, make
def consume(value: &Box[View]) -> i64:
    value.read()
def main() -> Result[(), AllocError | IoError]:
    value = make()?
    print(consume(&value))?
    Ok(())
"#,
    )
    .unwrap();
    plenty::check_file(&main, None).unwrap();
    std::fs::write(directory.path().join("api.plenty"), "protocol Hidden:\n    def read(self) -> i64:\n        pass\npub def expose(value: Box[Hidden]) -> ():\n    pass\n").unwrap();
    std::fs::write(&main, "import api\ndef main() -> ():\n    pass\n").unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(error.contains("private protocol"), "{error}");
}

#[test]
fn recursive_interface_signatures_report_a_diagnostic() {
    check_error(
        r#"
protocol A:
    def other(self) -> Box[B]:
        pass
protocol B:
    def other(self) -> Box[A]:
        pass
def take(value: Box[A]) -> ():
    pass
"#,
        "cannot box protocol",
    );
}

#[test]
fn projected_temporary_keeps_the_owner_alive() {
    run(
        &format!(
            r#"{TYPES}
print([Box[Counter](Count(15)).unwrap()].unwrap()[0].current()).unwrap()
"#
        ),
        "15\n",
    );
}

#[test]
fn two_phase_receiver_and_generic_enum_payload() {
    run(
        r#"
protocol Cell:
    def get(self) -> i64:
        pass
    def set(self: &mut Cell, n: i64) -> ():
        pass
enum Value[T]:
    Stored(T)
    def get(self) -> T:
        match self:
            case Value.Stored(n):
                *n
    def set(self: &mut Value[T], n: T) -> ():
        *self = Value[T].Stored(n)
mut value = Box[Cell](Value[i64].Stored(20)).unwrap()
value.set(value.get() + 1)
print(value.get()).unwrap()
"#,
        "21\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn failed_collection_insertion_consumes_the_new_box() {
    run(
        r#"
protocol Id:
    def id(self) -> i64:
        pass
class Guard:
    def id(self) -> i64:
        1
    def __del__(self) -> ():
        write_stdout("drop\n").unwrap()
        pass
mut values: list[Box[Id]] = [].unwrap()
value = Box[Id](Guard()).unwrap()
print("__test_fail_allocations_after_0__").unwrap()
match values.append(value):
    case Ok(_):
        write_stdout("unexpected\n").unwrap()
        pass
    case Err(_):
        write_stdout("failed\n").unwrap()
        pass
print("__test_restore_allocations__").unwrap()
print(len(values)).unwrap()
"#,
        "__test_fail_allocations_after_0__\ndrop\nfailed\n__test_restore_allocations__\n0\n",
    );
}

#[test]
fn matching_method_subset_does_not_change_erased_identity() {
    check_error(
        &format!(
            r#"{TYPES}
protocol Read:
    def current(self) -> i64:
        pass
def read[T: Read](value: &T) -> i64:
    value.current()
value = Box[Counter](Count(1)).unwrap()
print(read(&value)).unwrap()
"#
        ),
        "erased protocol identity differs",
    );
    for container in ["set[Box[Counter]]", "dict[Box[Counter], i64]"] {
        check_error(
            &format!("{TYPES}\ndef take(value: {container}) -> ():\n    pass\n"),
            "key",
        );
    }
}

#[test]
fn boxing_cannot_grant_private_method_access() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("api.plenty"),
        "pub protocol Read:\n    def read(self) -> i64:\n        pass\n",
    )
    .unwrap();
    let main = directory.path().join("main.plenty");
    std::fs::write(
        &main,
        r#"
from api import Read
class Local:
    def read(self) -> i64:
        1
def main() -> Result[(), AllocError]:
    value = Box[Read](Local())?
    Ok(())
"#,
    )
    .unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(error.contains("private"), "{error}");
}

#[test]
fn ineligible_requirements_still_work_through_static_bounds() {
    run(
        r#"
protocol Duplicate:
    def duplicate(self) -> Result[Box[Duplicate], AllocError]:
        pass
class Count:
    n: i64
    def duplicate(self) -> Result[Box[Count], AllocError]:
        Box(Count(self.n))
def duplicate[T: Duplicate](value: &T) -> Result[Box[T], AllocError]:
    value.duplicate()
value = Count(12)
result = duplicate(&value).unwrap()
print(result.n).unwrap()
"#,
        "12\n",
    );
}

#[test]
fn only_box_is_a_protocol_erasure_boundary() {
    check_error(
        &format!("{TYPES}\ndef take(value: Shared[Counter]) -> ():\n    pass\n"),
        "unknown type",
    );
}

#[test]
fn erasure_cannot_hide_wait_obligations_or_callback_captures() {
    for hidden in [
        "ThreadPoolExecutor",
        "Future[i64]",
        "BoxedCallable[[], i64]",
    ] {
        check_error(
            &format!(
                r#"
protocol Id:
    def id(self) -> i64:
        pass
class Hidden:
    value: {hidden}
    def id(self) -> i64:
        1
def erase(value: Hidden) -> Result[Box[Id], AllocError]:
    Box[Id](value)
"#
            ),
            "cannot conceal",
        );
    }
}

#[test]
fn erasure_rejects_nested_channel_endpoints() {
    for endpoint in ["Sender[i64]", "Receiver[i64]"] {
        for payload in [
            endpoint.to_owned(),
            format!("list[{endpoint}]"),
            format!("dict[str, Box[{endpoint}]]"),
            format!("Option[{endpoint}]"),
            "Nested".to_owned(),
            "Wrapped".to_owned(),
        ] {
            check_error(
                &format!(
                    r#"
protocol Id:
    def id(self) -> i64:
        pass
class Nested:
    endpoint: {endpoint}
enum Wrapped:
    Empty
    Held(Nested)
class Hidden:
    payload: {payload}
    def id(self) -> i64:
        1
def erase(value: Hidden) -> Result[Box[Id], AllocError]:
    Box[Id](value)
"#
                ),
                "channel endpoints",
            );
        }
        check_error(
            &format!(
                r#"
protocol Id:
    def id(self) -> i64:
        pass
enum Hidden:
    Empty
    Held({endpoint})
    def id(self) -> i64:
        1
def erase(value: Hidden) -> Result[Box[Id], AllocError]:
    Box[Id](value)
"#
            ),
            "channel endpoints",
        );
    }
}

const ENDPOINT_HOLDER: &str = r#"
protocol Id:
    def id(self) -> i64:
        pass
class Kept:
    endpoint: Receiver[i64]
    def id(self) -> i64:
        1
def produce(sender: Sender[i64]) -> ():
    for n in range(3):
        match sender.send(n):
            case Ok(_):
                pass
            case Err(_):
                return
"#;

#[test]
fn endpoint_erasure_cannot_bypass_any_pool_wait() {
    for wait in [
        "future.result().unwrap()",
        "pool.shutdown()",
        "drop(future)",
    ] {
        let source = format!(
            r#"{ENDPOINT_HOLDER}
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    kept = Box[Id](Kept(receiver)).unwrap()
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(produce, sender).unwrap()
        {wait}
    drop(kept)
"#
        );
        check_error(&source, "channel endpoints");
        // The concrete representation continues to expose the live endpoint.
        check_error(
            &source.replace("Box[Id](Kept(receiver))", "Box(Kept(receiver))"),
            "could block the job's send",
        );
    }
}

#[test]
fn concrete_endpoint_holder_can_be_dropped_before_waiting() {
    for wait in [
        "future.result().unwrap()",
        "pool.shutdown()",
        "drop(future)",
    ] {
        run(
            &format!(
                r#"{ENDPOINT_HOLDER}
def main() -> ():
    sender, receiver = channel[i64](1).unwrap()
    kept = Box(Kept(receiver)).unwrap()
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(produce, sender).unwrap()
        drop(kept)
        {wait}
    print("done").unwrap()
"#
            ),
            "done\n",
        );
    }
}
