//! Test-only markers must attribute allocation attempts to the calling thread.
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
        b"error: unexpected allocation in allocation-free region\n"
    );
}
