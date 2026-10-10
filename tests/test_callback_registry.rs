//! Stateful registries should compose ordinary records, borrows, and callables.
mod support;

#[test]
fn generic_stateful_registry_dispatches_and_preserves_independent_state() {
    let output = support::run(include_str!("../examples/callback_registry.plenty"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"13\n97\n15\n");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn registry_dispatch_uses_no_allocation_and_owned_state_drops_once() {
    let output = support::run(
        r#"
class State:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()
class Handler[S]:
    state: S
    callback: Callable[[&mut S], ()]
    def call(self: &mut Handler[S]) -> ():
        self.callback(&mut self.state)
def bump(state: &mut State) -> ():
    state.value = state.value + 1
mut handlers = [Handler(State(10), bump)].unwrap()
print("__test_begin_no_allocations__").unwrap()
print("__test_fail_allocations_after_0__").unwrap()
handlers[0].call()
handlers[0].call()
print("__test_restore_allocations__").unwrap()
print("__test_end_no_allocations__").unwrap()
drop(handlers)
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"__test_begin_no_allocations__\n__test_fail_allocations_after_0__\n__test_restore_allocations__\n__test_end_no_allocations__\n12\n");
}
