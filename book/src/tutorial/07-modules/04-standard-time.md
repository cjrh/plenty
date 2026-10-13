# Measure time and wait

Import `std.time` for spans, clock readings, and sleep. A `Duration` is a
nonnegative span; an `Instant` is a reading of a clock that never goes
backwards.

```plenty
from std.time import monotonic_now, sleep, milliseconds, seconds

def main() -> Result[(), Failure]:
    pause = milliseconds(5)
    start = monotonic_now()
    sleep(&pause)
    elapsed = monotonic_now().duration_since(&start)?
    print(elapsed.as_milliseconds() >= pause.as_milliseconds())?
    minutes = seconds(60).checked_mul(90)?
    print(minutes.as_seconds())?
    print(seconds(1) == milliseconds(1000))?
    Ok(())
```

```output
True
5400
True
```

`sleep` blocks for at least the span; a busy machine can make it longer, so the
example prints a comparison, not a measurement. Unit constructors take `u32`
counts and never fail. Arithmetic that could leave the representable range
returns `Result` with `RangeError`, including a subtraction that would be
negative:

```plenty
from std.time import milliseconds, seconds

def main() -> Result[(), Failure]:
    short = milliseconds(250)
    long = seconds(1)
    match short.checked_sub(&long):
        case Ok(span):
            print(span.as_milliseconds())?
        case Err(RangeError.OutOfRange):
            print("a span cannot be negative")?
    print(short.saturating_sub(&long).is_zero())?
    print(long.checked_sub(&short)?.as_milliseconds())?
    Ok(())
```

```output
a span cannot be negative
True
750
```

Time values are class instances, so they move. Pass them by reference, as these
calls do, and they stay usable.

## Wall-clock time is a different type

`wall_now()` returns a `Timestamp`: seconds and nanoseconds since the Unix
epoch. The system clock can be set backwards, so the difference between two
timestamps is fallible. This example builds two readings by hand to show a
clock that was stepped back by two seconds:

```plenty
from std.time import Timestamp, wall_now

def main() -> Result[(), Failure]:
    first = Timestamp(1700000010, 0)
    second = Timestamp(1700000008, 500000000)
    match second.duration_since(&first):
        case Ok(span):
            print(span.as_milliseconds())?
        case Err(RangeError.OutOfRange):
            print("the clock moved backwards")?
    print(first.duration_since(&second)?.as_milliseconds())?
    print(wall_now().seconds > first.seconds)?
    Ok(())
```

```output
the clock moved backwards
1500
True
```

Use instants to measure elapsed time and timestamps to record when something
happened. The compiler rejects mixing them:

```plenty-error
from std.time import monotonic_now, wall_now

def main() -> Result[(), Failure]:
    started = monotonic_now()
    print(wall_now().duration_since(&started)?.as_seconds())?
    Ok(())
```

```error
expected &std.time.Timestamp, got &std.time.Instant
```

The module has no calendar, time-zone, or formatting support. See the
[time reference](../../design/stdlib/time.md) for every operation, and
[Share one deadline across waits](../11-concurrency/deadlines.md) for timeouts.
