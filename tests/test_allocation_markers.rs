//! Test-only markers must attribute allocations to the calling thread. An
//! allocation-free region counts successful allocations only, so injected
//! failures inside it do not trip it.
#![cfg(feature = "runtime-checks")]

mod support;

#[test]
fn worker_allocations_do_not_trip_parent_region() {
    let output = support::run(
        r#"
def work() -> ():
    values = [1, 2, 3].unwrap()
    drop(values)
def main() -> Result[(), Failure]:
    print("__test_begin_no_allocations__")?
    with spawn(work)?:
        pass
    print("__test_end_no_allocations__")?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"__test_begin_no_allocations__\n__test_end_no_allocations__\n"
    );
}

#[test]
fn overlapping_regions_keep_each_threads_baseline() {
    let output = support::run(
        r#"
def work() -> ():
    values = [1, 2, 3].unwrap()
    drop(values)
    print("__test_begin_no_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
def main() -> Result[(), Failure]:
    print("__test_begin_no_allocations__")?
    with spawn(work)?:
        pass
    print("__test_end_no_allocations__")?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"__test_begin_no_allocations__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_end_no_allocations__\n"
    );
}

#[test]
fn worker_region_cannot_hide_parent_allocation_attempts() {
    let output = support::run(
        r#"
def work() -> ():
    print("__test_begin_no_allocations__").unwrap()
    print("__test_end_no_allocations__").unwrap()
def main() -> Result[(), Failure]:
    print("__test_begin_no_allocations__")?
    values = [1, 2, 3]?
    drop(values)
    with spawn(work)?:
        pass
    print("__test_end_no_allocations__")?
    Ok(())
"#,
    );
    assert!(!output.status.success());
    assert_eq!(
        output.stderr,
        b"error: unexpected successful allocation in allocation-free region\n"
    );
}

#[test]
fn injected_failures_inside_a_region_are_not_allocations() {
    let output = support::run(
        r#"
def main() -> Result[(), Failure]:
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    values = [1, 2, 3]
    pairs = {1: 2}
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(values.is_err(), pairs.is_err())?
    Ok(())
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"__test_fail_allocations_after_0__\n__test_begin_no_allocations__\n__test_end_no_allocations__\n__test_restore_allocations__\nTrue True\n"
    );
}

#[test]
fn successes_within_a_failure_budget_still_trip_a_region() {
    let output = support::run(
        r#"
def main() -> Result[(), Failure]:
    print("__test_fail_allocations_after_1__")?
    print("__test_begin_no_allocations__")?
    # The first allocation succeeds and the rest fail; only the success counts.
    first = [1, 2, 3]
    second = [4, 5, 6]
    print("__test_end_no_allocations__")?
    print(first.is_ok(), second.is_err())?
    Ok(())
"#,
    );
    assert!(!output.status.success());
    assert_eq!(
        output.stderr,
        b"error: unexpected successful allocation in allocation-free region\n"
    );
}
