mod support;
use support::run;

#[test]
fn temporary_callees_are_evaluated_once_before_arguments() {
    let output = run(r#"
def make(start: i64) -> Closure[[i64], i64]:
    print(start).unwrap()
    def [mut start](step: i64) -> i64:
        start = start + step
        start
def argument() -> i64:
    print(2).unwrap()
    3
def main() -> Result[(), Failure]:
    print(make(1)(argument()))?
    print(1 + make(10)(4))?
    wrapped = Some(make(20))
    print(wrapped.unwrap()(5))?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1\n2\n4\n10\n15\n20\n25\n"
    );
}

#[test]
fn argument_propagation_cleans_pending_values_and_temporary_environments() {
    let output = run(r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def make() -> Result[Closure[[Resource, i64], i64], AllocError]:
    resource = Resource(1)
    f = def [resource](argument: Resource, value: i64) -> i64:
        resource.id + argument.id + value
    Ok(f)
def fail() -> Result[i64, AllocError]:
    Err(AllocError.OutOfMemory)
def work() -> Result[(), AllocError]:
    make()?(Resource(2), fail()?)
    Ok(())
def main() -> Result[(), Failure]:
    print(work())?
    Ok(())
"#);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "2\n1\nResult[(), AllocError].Err(AllocError.OutOfMemory)\n"
    );
}
