//! std.time: clock arithmetic is checked against caller-built instants and
//! timestamps, so no assertion depends on how long this machine takes. Real
//! clocks are only held to lower bounds and ordering.

fn compile(source: &str) -> Result<(tempfile::TempDir, std::path::PathBuf), String> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.plenty");
    let executable = directory.path().join("program");
    std::fs::write(&path, source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .map_err(|error| error.to_string())?;
    Ok((directory, executable))
}

fn runs(source: &str, expected: &str) {
    let (_directory, executable) = compile(source).unwrap_or_else(|e| panic!("{source}\n{e}"));
    // A lost wakeup would otherwise hang the suite instead of failing it.
    let output = std::process::Command::new("timeout")
        .arg("60s")
        .arg(executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status {:?}; stdout {}; stderr {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let visible = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(visible, expected.trim(), "{source}");
}

fn rejected(source: &str, expected: &str) {
    let error = compile(source).expect_err(source);
    assert!(error.contains(expected), "{error}");
}

#[test]
fn durations_convert_and_report_every_unrepresentable_result() {
    runs(
        r#"
from std.time import Duration, nanoseconds, microseconds, milliseconds, seconds

def show(result: Result[Duration, RangeError]) -> ():
    match result:
        case Ok(span):
            print(span.as_nanoseconds()).unwrap()
        case Err(RangeError.OutOfRange):
            print("out of range").unwrap()

def main() -> Result[(), AllocError | IoError | RangeError]:
    span = seconds(2).checked_add(milliseconds(1500))?
    print(span.as_seconds(), span.as_milliseconds(), span.as_microseconds(), span.as_nanoseconds())?
    print(seconds(1) == milliseconds(1000), microseconds(1) == nanoseconds(1000), seconds(1) == seconds(2))?
    print(seconds(u32.MAX).as_seconds(), milliseconds(u32.MAX).as_milliseconds(), microseconds(u32.MAX).as_microseconds())?
    print(nanoseconds(0).is_zero(), nanoseconds(1).is_zero())?
    largest = nanoseconds(u64.MAX)
    show(largest.checked_add(nanoseconds(0)))
    show(largest.checked_add(nanoseconds(1)))
    show(seconds(1).checked_sub(seconds(1)))
    show(seconds(1).checked_sub(nanoseconds(1000000001)))
    print(seconds(1).saturating_sub(seconds(3)).is_zero(), seconds(3).saturating_sub(seconds(1)).as_seconds())?
    show(seconds(3).checked_mul(0))
    show(nanoseconds(0).checked_mul(u64.MAX))
    show(nanoseconds(u64.MAX).checked_mul(1))
    show(nanoseconds(u64.MAX // 2 + 1).checked_mul(2))
    show(seconds(60).checked_mul(60))
    Ok(())
"#,
        "
3 3500 3500000 3500000000
True True False
4294967295 4294967295 4294967295
True False
18446744073709551615
out of range
0
out of range
True 2
0
0
18446744073709551615
out of range
3600000000000
",
    );
}

#[test]
fn millisecond_budgets_round_up_and_never_wrap() {
    runs(
        r#"
from std.time import nanoseconds, milliseconds

def main() -> Result[(), AllocError | IoError]:
    print(nanoseconds(0).as_timeout_ms(), nanoseconds(1).as_timeout_ms(), nanoseconds(999999).as_timeout_ms())?
    print(nanoseconds(1000000).as_timeout_ms(), nanoseconds(1000001).as_timeout_ms(), milliseconds(7).as_timeout_ms())?
    print(nanoseconds(u64.MAX).as_timeout_ms(), nanoseconds(u64.MAX).as_milliseconds())?
    Ok(())
"#,
        "
0 1 1
1 2 7
18446744073710 18446744073709
",
    );
}

#[test]
fn fake_monotonic_clock_drives_zero_expired_and_overflowing_deadlines() {
    runs(
        r#"
from std.time import Deadline, Instant, instant_at, deadline_at, deadline_after, nanoseconds, seconds

def report(deadline: &Deadline, tick: u64) -> ():
    now = instant_at(tick)
    print(tick, deadline.expired(&now), deadline.remaining(&now).as_nanoseconds()).unwrap()

def main() -> Result[(), AllocError | IoError | RangeError]:
    start = instant_at(1000)
    deadline = deadline_after(&start, &nanoseconds(500))?
    for tick in [0u64, 1000u64, 1499u64, 1500u64, 1501u64, u64.MAX]?:
        report(&deadline, tick)
    zero = deadline_after(&start, &nanoseconds(0))?
    report(&zero, 999)
    report(&zero, 1000)
    fixed = deadline_at(&instant_at(u64.MAX))
    report(&fixed, u64.MAX - 1)
    report(&fixed, u64.MAX)
    late = instant_at(u64.MAX - 5)
    print(str.repr(deadline_after(&late, &nanoseconds(6)))?)?
    report(&deadline_after(&late, &nanoseconds(5))?, u64.MAX - 5)
    print(str.repr(late.checked_add(seconds(1)))?)?
    moved = start.checked_add(nanoseconds(250))?
    print(moved.duration_since(&start)?.as_nanoseconds(), moved == instant_at(1250))?
    print(str.repr(start.duration_since(&moved))?)?
    print(start.saturating_duration_since(&moved).is_zero(), moved.saturating_duration_since(&start).as_nanoseconds())?
    Ok(())
"#,
        "
0 False 1500
1000 False 500
1499 False 1
1500 True 0
1501 True 0
18446744073709551615 True 0
999 False 1
1000 True 0
18446744073709551614 False 1
18446744073709551615 True 0
Result[std.time.Deadline, RangeError].Err(RangeError.OutOfRange)
18446744073709551610 False 5
Result[std.time.Instant, RangeError].Err(RangeError.OutOfRange)
250 True
Result[std.time.Duration, RangeError].Err(RangeError.OutOfRange)
True 250
",
    );
}

#[test]
fn fake_wall_clock_jumps_are_errors_not_negative_or_wrapped_spans() {
    runs(
        r#"
from std.time import Duration, Timestamp, nanoseconds, milliseconds, seconds

def since(later: &Timestamp, earlier: &Timestamp) -> ():
    match later.duration_since(earlier):
        case Ok(span):
            print(span.as_nanoseconds()).unwrap()
        case Err(RangeError.OutOfRange):
            print("out of range").unwrap()

def main() -> Result[(), AllocError | IoError | RangeError]:
    before = Timestamp(1700000000, 900000000)
    after = Timestamp(1700000002, 100000000)
    since(&after, &before)
    # The system clock was stepped back between two readings.
    since(&before, &after)
    since(&before, &before)
    since(&Timestamp(5, 1), &Timestamp(5, 2))
    since(&Timestamp(5, 2), &Timestamp(5, 1))
    # Floor seconds keep ordering across the epoch.
    since(&Timestamp(1, 250000000), &Timestamp(-1, 500000000))
    since(&Timestamp(-1, 500000000), &Timestamp(-2, 750000000))
    # Spans of more than 584 years do not fit, in either sign combination.
    since(&Timestamp(i64.MAX, 0), &Timestamp(i64.MIN, 0))
    since(&Timestamp(18446744073, 709551615), &Timestamp(0, 0))
    since(&Timestamp(18446744073, 709551616), &Timestamp(0, 0))
    since(&Timestamp(i64.MIN + 18446744073, 709551615), &Timestamp(i64.MIN, 0))
    # Public fields can hold a sub-second part no clock produces.
    since(&Timestamp(9, 1000000000), &Timestamp(1, 0))
    since(&Timestamp(9, 0), &Timestamp(1, 1000000000))
    print(Timestamp(-1, 500000000).checked_add(milliseconds(750))?)?
    print(Timestamp(10, 999999999).checked_add(nanoseconds(1))?)?
    print(Timestamp(i64.MAX - 1, 999999999).checked_add(nanoseconds(1))?)?
    print(str.repr(Timestamp(i64.MAX, 999999999).checked_add(nanoseconds(1)))?)?
    print(str.repr(Timestamp(i64.MAX - 3, 0).checked_add(seconds(4)))?)?
    print(Timestamp(i64.MIN, 0).checked_add(nanoseconds(u64.MAX))?)?
    print(str.repr(Timestamp(0, 1000000000).checked_add(nanoseconds(0)))?)?
    Ok(())
"#,
        "
1200000000
out of range
0
out of range
1
1750000000
750000000
out of range
18446744073709551615
out of range
18446744073709551615
out of range
out of range
std.time.Timestamp(seconds=0, nanoseconds=250000000)
std.time.Timestamp(seconds=11, nanoseconds=0)
std.time.Timestamp(seconds=9223372036854775807, nanoseconds=0)
Result[std.time.Timestamp, RangeError].Err(RangeError.OutOfRange)
Result[std.time.Timestamp, RangeError].Err(RangeError.OutOfRange)
std.time.Timestamp(seconds=-9223372018408031735, nanoseconds=709551615)
Result[std.time.Timestamp, RangeError].Err(RangeError.OutOfRange)
",
    );
}

#[test]
fn real_clocks_only_promise_order_and_minimum_sleep() {
    runs(
        r#"
from std.time import monotonic_now, wall_now, sleep, sleep_until, deadline_after, deadline_at, nanoseconds, milliseconds

def main() -> Result[(), AllocError | IoError | RangeError]:
    first = monotonic_now()
    second = monotonic_now()
    print(second.duration_since(&first)?.as_nanoseconds() >= 0u64)?
    sleep(nanoseconds(0))
    sleep(milliseconds(3))
    third = monotonic_now()
    print(third.duration_since(&second)?.as_nanoseconds() >= 3000000u64)?
    deadline = deadline_after(&third, &milliseconds(3))?
    print(deadline.expired(&third))?
    sleep_until(&deadline)
    print(deadline.expired(&monotonic_now()), deadline.remaining(&monotonic_now()).is_zero())?
    # A deadline already in the past returns without sleeping.
    sleep_until(&deadline_at(&first))
    now = wall_now()
    print(now.seconds > 1700000000, now.nanoseconds < 1000000000u32)?
    Ok(())
"#,
        "
True
True
False
True True
True True
",
    );
}

#[test]
fn one_deadline_bounds_existing_timed_waits_and_cancellation_wakes_them() {
    runs(
        r#"
from std.time import Deadline, monotonic_now, deadline_after, deadline_at, instant_at, seconds

def wait(stop: CancellationToken, deadline: Deadline, ready: Sender[i64]) -> bool:
    ready.send(1).unwrap()
    # An hour-long budget: only cancellation can end this wait in time.
    stop.wait_timeout(deadline.remaining(&monotonic_now()).as_timeout_ms())

def main() -> Result[(), AllocError | IoError | ChannelError | SendTimeoutError[i64] | RecvTimeoutError | RangeError | PoolError | FutureError | RecvError | SubmitError[OnceClosure[[], bool]]]:
    stop = CancellationToken()?
    past = deadline_at(&instant_at(0))
    budget = past.remaining(&monotonic_now()).as_timeout_ms()
    print(budget, stop.wait_timeout(budget))?
    sender, receiver = channel[i64](1)?
    match receiver.recv_timeout(budget):
        case Err(RecvTimeoutError.TimedOut):
            print("timed out")?
        case _:
            print("unexpected")?
    sender.send_timeout(7, budget)?
    # Readiness wins over an expired deadline.
    print(receiver.recv_timeout(budget)?)?
    drop(sender)
    drop(receiver)
    hour = deadline_after(&monotonic_now(), &seconds(3600))?
    with ThreadPoolExecutor(1, 1)? as pool:
        ready, started = channel[i64](1)?
        future = pool.submit(wait, stop.share(), hour, ready)?
        started.recv()?
        drop(started)
        print(future.wait_timeout(0))?
        stop.cancel()
        print(future.result()?)?
    Ok(())
"#,
        "
0 False
timed out
7
False
True
",
    );
}

#[test]
fn workers_may_read_clocks_and_sleep() {
    runs(
        r#"
from std.time import monotonic_now, wall_now, sleep, milliseconds

def nap(count: u32) -> bool:
    start = monotonic_now()
    sleep(milliseconds(count))
    wall = wall_now()
    wall.nanoseconds < 1000000000u32 and monotonic_now().duration_since(&start).unwrap().as_milliseconds() >= u64(count)

def main() -> Result[(), AllocError | IoError | PoolError | FutureError | ThreadError | SubmitError[OnceClosure[[], bool]]]:
    with ThreadPoolExecutor(2, 2)? as pool:
        first = pool.submit(nap, 2u32)?
        second = pool.submit(nap, 1u32)?
        print(first.result()?, second.result()?)?
    with spawn(nap, 1u32)? as task:
        pass
    Ok(())
"#,
        "True True",
    );
}

#[test]
fn time_values_clocks_and_sleep_allocate_nothing() {
    runs(
        r#"
from std.time import monotonic_now, wall_now, sleep, sleep_until, deadline_after, nanoseconds, milliseconds, seconds, Timestamp

def main() -> Result[(), AllocError | IoError]:
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    start = monotonic_now()
    span = seconds(1).checked_add(milliseconds(5)).unwrap().checked_mul(3).unwrap()
    deadline = deadline_after(&start, &nanoseconds(1)).unwrap()
    sleep(nanoseconds(1))
    sleep_until(&deadline)
    left = deadline.remaining(&monotonic_now()).as_timeout_ms()
    wall = wall_now()
    later = wall.checked_add(&span).unwrap()
    apart = later.duration_since(&wall).unwrap()
    print("__test_restore_allocations__")?
    print("__test_end_no_allocations__")?
    print(left, apart.as_milliseconds(), apart == span)?
    Ok(())
"#,
        "0 3015 True",
    );
}

#[test]
fn filesystem_metadata_reports_std_time_timestamps() {
    runs(
        r#"
from std.fs import metadata
from std.path import from_text
from std.time import Timestamp, seconds

def shifted(stamp: &Timestamp) -> Result[Timestamp, RangeError]:
    stamp.checked_add(seconds(1))

def main() -> Result[(), AllocError | IoError | RangeError]:
    data = metadata(&from_text("/")?)?
    changed = data.unix_changed()
    print(shifted(&changed)?.duration_since(&changed)?.as_seconds())?
    Ok(())
"#,
        "1",
    );
}

#[test]
fn clock_kinds_and_private_representations_do_not_mix() {
    rejected(
        r#"
from std.time import monotonic_now, wall_now

def main() -> Result[(), AllocError | IoError]:
    start = monotonic_now()
    print(start.duration_since(&wall_now())?.as_seconds())?
    Ok(())
"#,
        "Timestamp",
    );
    rejected(
        r#"
from std.time import Duration

def main() -> ():
    span = Duration(5)
"#,
        "private",
    );
    rejected(
        r#"
from std.time import Instant

def main() -> ():
    start = Instant(5)
"#,
        "private",
    );
    rejected(
        r#"
from std.time import seconds, monotonic_now

def main() -> Result[(), AllocError | IoError]:
    print(monotonic_now().checked_add(5)?)?
    Ok(())
"#,
        "Duration",
    );
    rejected(
        r#"
from std.time import seconds

def main() -> ():
    span = seconds(-1)
"#,
        "u32",
    );
}
