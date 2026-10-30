# std.time

`std.time` provides nonnegative spans, monotonic instants, deadlines, and
wall-clock timestamps as four distinct types, with clock reads and sleep. The
arithmetic is ordinary embedded Plenty code. The runtime supplies only two
clock reads and one sleep. No operation allocates.

Every time value is a small [class](../language/03-types/classes.md), so it is
affine: pass it by reference, as every operation here does, and use `copy` for
an independent value. `==` compares values structurally. Ordering has no
operator; compare `as_nanoseconds()` results or use the operations below.

```plenty
from std.time import deadline_after, instant_at, milliseconds, monotonic_now, seconds, sleep

def main() -> Result[(), RangeError | IoError]:
    start = monotonic_now()
    sleep(&milliseconds(20))
    elapsed = monotonic_now().duration_since(&start)?
    print(elapsed.as_milliseconds() >= 20)?

    # A fake timeline makes deadline arithmetic exact.
    deadline = deadline_after(&instant_at(0), &seconds(5))?
    later = instant_at(seconds(2).as_nanoseconds())
    print(deadline.remaining(&later).as_milliseconds(), deadline.expired(&later))?
    Ok(())
```
```output
True
3000 False
```

## Durations

A `Duration` is a span of `u64` nanoseconds, from zero to just over 584 years.
A negative span is unrepresentable.

| Operation | Result | Contract |
|---|---|---|
| `nanoseconds(count: u64)` | `Duration` | Exact |
| `microseconds(count: u32)`, `milliseconds(count: u32)`, `seconds(count: u32)` | `Duration` | Exact; a `u32` count always fits |
| `span.as_nanoseconds()` | `u64` | Exact |
| `span.as_microseconds()`, `as_milliseconds()`, `as_seconds()` | `u64` | Whole units, discarding the remainder |
| `span.as_timeout_ms()` | `u64` | Milliseconds rounded up |
| `span.is_zero()` | `bool` | |
| `span.checked_add(&other)` | `Result[Duration, RangeError]` | Error above the maximum |
| `span.checked_sub(&other)` | `Result[Duration, RangeError]` | Error when `other` is longer |
| `span.saturating_sub(&other)` | `Duration` | Zero when `other` is longer |
| `span.checked_mul(factor: u64)` | `Result[Duration, RangeError]` | Error above the maximum |

Constructors never fail and arithmetic never traps or wraps. Build a span of
minutes or longer by multiplying, as in `seconds(60).checked_mul(5)`. There is
no float conversion, division, or text format.

## Instants and deadlines

An `Instant` is a point on a monotonic timeline: it never moves backwards and
is unaffected by changes to the system clock. Its origin is unspecified, so an
instant means something only relative to another from the same timeline.

| Operation | Result | Contract |
|---|---|---|
| `monotonic_now()` | `Instant` | Read the process's monotonic clock |
| `instant_at(nanoseconds_after_origin: u64)` | `Instant` | A point on a caller-defined timeline |
| `at.checked_add(&span)` | `Result[Instant, RangeError]` | Error past the end of the timeline |
| `at.duration_since(&earlier)` | `Result[Duration, RangeError]` | Error when `earlier` is later |
| `at.saturating_duration_since(&earlier)` | `Duration` | Zero when `earlier` is later |

A `Deadline` is the instant at which a total budget ends. Code that receives a
`&Deadline` can make several waits without restarting the budget.

| Operation | Result | Contract |
|---|---|---|
| `deadline_after(&start, &budget)` | `Result[Deadline, RangeError]` | Error past the end of the timeline |
| `deadline_at(&at)` | `Deadline` | |
| `deadline.expired(&now)` | `bool` | True once `now` reaches the deadline |
| `deadline.remaining(&now)` | `Duration` | Zero once expired |

A zero budget is expired at its own start. Deadline operations take the current
instant as an argument and read no clock, so their results depend only on their
inputs. `instant_at` supplies the fake clock for simulations and tests; mixing
its instants with `monotonic_now()` readings is meaningless and undetected.

The monotonic clock is Linux `CLOCK_MONOTONIC`, reported as nanoseconds since
the process's first reading. It does not advance while the machine is
suspended. Readings are comparable across threads of one process and not
across processes.

## Clocks

`Clock` is the protocol for code that reads the time through a value instead of
calling `monotonic_now()`, so a test can control what it sees:

```text
pub protocol Clock:
    def now(self) -> Instant:
        pass
```

`monotonic_clock()` returns a `MonotonicClock`, whose `now()` is
`monotonic_now()`; it holds no state. [`std.test.fake_clock`](test.md#test-doubles)
returns a clock that moves only when told. A function written as
`def f[C: Clock](clock: &C)` is compiled once per clock type, with no dynamic
dispatch.

## Wall-clock timestamps

A `Timestamp` has public fields `seconds: i64`, counted from the Unix epoch in
UTC, and `nanoseconds: u32` in `[0, 1_000_000_000)`. Times before the epoch use
floor seconds: half a second before it is `Timestamp(-1, 500000000)`.
[`std.fs` metadata](fs/metadata.md) returns this type.

| Operation | Result | Contract |
|---|---|---|
| `wall_now()` | `Timestamp` | Read the system clock once |
| `Timestamp(seconds, nanoseconds)` | `Timestamp` | Unchecked field construction |
| `stamp.duration_since(&earlier)` | `Result[Duration, RangeError]` | Error when `earlier` is later, or the span exceeds a Duration |
| `stamp.checked_add(&span)` | `Result[Timestamp, RangeError]` | Error past `i64` seconds |

The system clock can be stepped in either direction, by an administrator or by
time synchronization. A later reading may therefore be earlier, and
`duration_since` reports that as an error instead of a negative or wrapped
span. Measure elapsed time and timeouts with instants. A timestamp cannot be
converted to an instant or compared with one.

A timestamp whose `nanoseconds` is out of range is an error in both operations.
There is no subtraction of a span, leap-second table, parsing, or formatting
here; the `tz` package provides calendars, zones, and text.

## Civil time and time zones

Calendar dates, wall-clock times, IANA time zones, and RFC 3339 text live in
the official [`tz` package](https://github.com/plentylang/tz), not in `std`.
It bundles a pinned release of the IANA time-zone database as Plenty source,
so results do not depend on the host's zone files, and a program that never
looks up a zone links no zone data. Its API follows Rust's jiff. Add it as a
[Git dependency](../toolchain/01-building/packages.md#git-dependencies):

```toml
[dependencies]
tz = { git = "https://github.com/plentylang/tz.git", tag = "v0.2.0" }
```

A `tz.Zoned` holds a `Timestamp` with its zone. Adding calendar units keeps
the wall-clock reading; a local time that a transition skips or repeats is
resolved by the `tz.Disambiguation` the caller passes:

```plenty fragment
import tz
from std.fmt import FormatError

def main() -> Result[(), tz.TzError | FormatError | AllocError | IoError]:
    new_york = tz.zone("America/New_York")?
    london = tz.zone("Europe/London")?
    meeting = tz.datetime(2024, 3, 9, 9, 30)?.to_zoned(&new_york, tz.Disambiguation.Reject)?
    next_week = meeting.checked_add(&tz.span(weeks=1), tz.Disambiguation.Reject)?
    print(f"{next_week}"?)?
    print(f"{next_week.in_zone(&london)?}"?)?
    print(meeting.span_until(&next_week).hours())?
    Ok(())
```

```text
2024-03-16T09:30:00-04:00[America/New_York]
2024-03-16T13:30:00+00:00[Europe/London]
167
```

`tz.zoned(&stamp, &zone)` and `zoned.timestamp()` convert between the two
types.

`tz.zone(name)` can return any zone, so it links the whole database (about
150 KB). The `tz.zones` module has one function per canonical zone, and each
links only that zone's data. The functions cannot fail:

```plenty fragment
import tz
from tz.zones import america_new_york

def main() -> Result[(), tz.TzError | AllocError | IoError]:
    new_york = america_new_york()
    meeting = tz.datetime(2024, 3, 10, 2, 30)?.to_zoned(&new_york, tz.Disambiguation.Compatible)?
    print(meeting.offset().seconds())?
    Ok(())
```

This prints `-14400`: 02:30 falls in that day's gap, and `Compatible` moves it
forward to 03:30 EDT. The package README lists the API, how zone names map to
function names, its data version, and how the data is regenerated.

## Sleeping and timed waits

| Operation | Result | Contract |
|---|---|---|
| `sleep(&span)` | `()` | Block this thread for at least the span |
| `sleep_until(&deadline)` | `()` | Block until the deadline; return at once if expired |

Sleep measures the monotonic clock, resumes after signals, and cannot be
cancelled. Scheduling may extend it; nothing shortens it. Workers may sleep and
read both clocks.

Existing [timed waits](../facilities/03-concurrency/timed-selection.md) keep
their `u64` millisecond budgets. `as_timeout_ms()` is the one conversion to
them. It rounds up so that a wait never ends before its deadline and a positive
remainder never becomes a zero-budget retry loop.

- A cancellable sleep is
  `token.wait_timeout(span.as_timeout_ms())` on a
  [cancellation token](../facilities/03-concurrency/cancellation.md). It returns
  true if cancellation was requested first. `std.time` adds no second token.
- A wait bounded by a shared deadline passes
  `deadline.remaining(&monotonic_now()).as_timeout_ms()` to `recv_timeout`,
  `send_timeout`, `select_recv_timeout`, or a future or token `wait_timeout`.

An expired deadline yields a zero budget, which those operations define as an
immediate check where readiness still wins.

## Errors, costs, and limits

Every recoverable failure is the built-in
[`RangeError.OutOfRange`](../facilities/01-builtins/numbers.md#checked-conversion):
the result is not representable in its type. Time arithmetic therefore
propagates with `?` alongside `T.try_from`. Clock reads and sleep cannot fail.

Each operation takes constant time and allocates nothing, including clock reads
and sleep. Values hold no OS resource and may move between threads.

Supported target: `x86_64-unknown-linux-gnu`. See the lessons
[Measure time and wait](../../tutorial/07-modules/04-standard-time.md) and
[Share one deadline across waits](../../tutorial/11-concurrency/deadlines.md).

## Design decisions

- **Four types, no conversions between clocks.** A span, a monotonic point, a
  budget end, and a wall time have different valid operations. Distinct types
  reject using a wall time for a timeout at compile time.
- **Integer nanoseconds.** One exact representation with no rounding rule.
  Unit constructors take `u32` so that construction is infallible; arithmetic
  is where overflow can occur, and it is always checked.
- **No hidden clock reads.** `expired` and `remaining` take `now`. A function's
  clock reads are visible at its call sites, and a fake clock needs no
  protocol, global state, or test-only mode.
- **Explicit rounding names.** `as_milliseconds` discards and `as_timeout_ms`
  rounds up. Neither is a default.
- **Existing waits are unchanged.** Millisecond budgets, cancellation tokens,
  and their wakeup rules remain the single mechanism; this module supplies the
  arithmetic that feeds them.
- **One timestamp type.** `std.fs` reuses `Timestamp` instead of declaring its
  own, so file times and clock readings subtract directly.

## Acknowledgements

- `Duration`, `Instant` and their checked and saturating operations are modelled on Rust's [`std::time`](https://doc.rust-lang.org/std/time/index.html).
