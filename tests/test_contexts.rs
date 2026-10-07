mod support;
use std::process::Command;

fn run(source: &str, expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("program");
    support::compile_source_to_executable(source, &executable)
        .unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = Command::new(executable).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn reject(source: &str, message: &str) {
    let directory = tempfile::tempdir().unwrap();
    let error = support::compile_source_to_executable(source, &directory.path().join("program"))
        .unwrap_err()
        .to_string();
    assert!(error.contains(message), "expected {message}, got {error}");
}

const MANAGER: &str = r#"
class Resource:
    name: str
    def __del__(self) -> ():
        print(self.name)
class Manager:
    name: str
    def __enter__(self: &mut Manager) -> Resource:
        print(self.name)
        Resource("entry dropped")
    def __exit__(self: &mut Manager) -> ():
        print("exit")
    def __del__(self) -> ():
        print("manager dropped")
"#;

#[test]
fn multiple_managers_enter_left_to_right_and_share_prior_bindings() {
    run(&format!(r#"{MANAGER}
with Manager("first") as first, Manager(first.name) as second:
    print("body")
"#), "first\nentry dropped\nbody\nentry dropped\nexit\nmanager dropped\nentry dropped\nexit\nmanager dropped\n");
}

#[test]
fn later_acquisition_failure_exits_only_entered_managers() {
    run(
        &format!(
            r#"{MANAGER}
def acquire() -> Option[Manager]:
    Nothing
def work() -> Option[i64]:
    with Manager("first") as first, acquire()? as second:
        print("unreachable")
    Some(1)
print(work())
"#
        ),
        "first\nentry dropped\nexit\nmanager dropped\nOption[i64].Nothing\n",
    );
}

#[test]
fn scope_cleanup_and_owned_entry() {
    run(
        &format!(
            r#"{MANAGER}
with Manager("enter") as resource:
    local = Resource("body dropped")
    print(resource.name)
print("after")
"#
        ),
        "enter\nentry dropped\nbody dropped\nentry dropped\nexit\nmanager dropped\nafter\n",
    );
}

#[test]
fn early_return_preserves_value_and_cleanup_order() {
    run(
        &format!(
            r#"{MANAGER}
def obtain() -> Resource:
    with Manager("enter") as resource:
        return resource
r = obtain()
print("returned")
"#
        ),
        "enter\nexit\nmanager dropped\nreturned\nentry dropped\n",
    );
}

#[test]
fn propagation_cleans_nested_contexts_and_pending_operands() {
    run(&format!(r#"{MANAGER}
def fail() -> Result[i64, i64]:
    Err(9)
def work() -> Result[i64, i64]:
    with Manager("outer") as a:
        with Manager("inner") as b:
            value = fail()?
    Ok(1)
print(work())
"#), "outer\ninner\nentry dropped\nexit\nmanager dropped\nentry dropped\nexit\nmanager dropped\nResult[i64, i64].Err(9)\n");
}

#[test]
fn loop_exits_cleanup_once_and_inner_loop_keeps_manager() {
    run(&format!(r#"{MANAGER}
for n in range(2):
    with Manager("enter") as r:
        if n == 0:
            continue
        while True:
            break
        break
"#), "enter\nentry dropped\nexit\nmanager dropped\nenter\nentry dropped\nexit\nmanager dropped\n");
}

#[test]
fn unit_entry_and_missing_as() {
    run(
        r#"
class Manager:
    def __enter__(self: &mut Manager) -> ():
        print("enter")
    def __exit__(self: &mut Manager) -> ():
        print("exit")
with Manager():
    print("body")
"#,
        "enter\nbody\nexit\n",
    );
    run(
        &format!("{MANAGER}\nwith Manager(\"enter\"):\n    print(\"body\")\n"),
        "enter\nentry dropped\nbody\nexit\nmanager dropped\n",
    );
}

#[test]
fn invalid_contexts_are_rejected() {
    reject("with 1:\n    pass\n", "with requires an owned class");
    reject(
        "class Empty:\n    value: i64\nwith Empty(1):\n    pass\n",
        "requires __enter__",
    );
    reject(
        &format!("{MANAGER}\nwith Manager(\"x\") as r:\n    pass\nprint(r)\n"),
        "unknown binding",
    );
    reject(
        &format!(
            "{MANAGER}\ndef gen() -> Generator[i64]:\n    with Manager(\"x\"):\n        yield 1\n"
        ),
        "yield inside with",
    );
    reject("class Bad:\n    def __enter__(self) -> ():\n        pass\n    def __exit__(self: &mut Bad) -> ():\n        pass\nwith Bad():\n    pass\n", "__enter__ requires only self: &mut");
}
