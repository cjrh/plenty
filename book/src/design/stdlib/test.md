# std.test

`import std.test` provides assertions, fixtures, and test doubles for the
tests that [`plenty test`](../toolchain/01-building/packages.md#tests) runs,
with one error type, `TestError`. The module is checked Plenty code with no
native primitives. Any program may import it; it does not depend on the runner except
for reading `PLENTY_TEST_SEED`.

## Assertions

An assertion that holds returns `Ok(())` without allocating. One that fails
flushes standard output, writes `assertion failed: ` and an explanation to
standard error, and returns `Err(TestError.Assertion)`. The explanation is
written at once because `?` usually erases the error into `Failure`.

| Function | Passes when | Failure explanation |
|---|---|---|
| `fail(message: &str)` | never | `message` |
| `check(condition: bool, message: &str)` | `condition` | `message` |
| `equal[T](left: &T, right: &T)` | `left == right` | `left == right`, then `  left: ` and ` right: ` lines |
| `not_equal[T](left: &T, right: &T)` | `left != right` | `left != right`, then a `  both: ` line |
| `less[T](left: &T, right: &T)` | `left < right` | `left < right`, then `left` and `right` lines |
| `close(left: f64, right: f64, tolerance: f64)` | `abs(left - right) <= tolerance` | the three values; NaN on either side fails |
| `is_ok[T, E](result: &Result[T, E])` | `result` is `Ok` | `result is Ok`, then an `  error: ` line |
| `is_err[T, E](result: &Result[T, E])` | `result` is `Err` | `result is Err`, then a `  value: ` line |

Values are shown with `str.repr`, so `T` must support equality or ordering as
the function needs and have a `repr`. Showing a value allocates; when that
fails the line reads `<no memory to show the value>`. A failure to write to
standard error is ignored.

Assertions do not report the caller's source location: Plenty has no
caller-location intrinsic. The runner names the failing test instead.

Under `plenty test`, a test that returns `Err(TestError.Assertion)`, or the
`Failure.Unspecified` that `?` makes of it, gets no further `error:` line: the
explanation above is the whole report.

## Fixtures

| Function | Result | Contract |
|---|---|---|
| `scratch()` | `Result[Scratch, TestError]` | A new empty directory named `plenty-test-` plus 32 random hex digits, created with mode `0700` under `$TMPDIR` when it is an absolute path, otherwise under `/tmp` |
| `seed()` | `Result[u64, TestError]` | `PLENTY_TEST_SEED` as a decimal `u64`, or 0 when it is unset; any other value writes a message to standard error and returns `TestError.InvalidSeed` |
| `rng()` | `Result[Rng, TestError]` | `std.random.seeded(seed()?)` |

`plenty test --seed N` sets `PLENTY_TEST_SEED` for every test it runs, so a
failure seen under one seed repeats under it.

`Scratch` owns its directory:

| Method | Result | Contract |
|---|---|---|
| `path()` | `&Path` | The directory's absolute path |
| `root()` | `&Root` | A [`std.fs.Root`](fs.md) anchored at the directory |
| `file(name: &str)` | `Result[str, TestError]` | The absolute path text of `name` inside the directory, for builtins such as `write_text` and `open` |
| `cleanup()` | `Result[(), TestError]` | Remove the directory and everything in it; later calls do nothing. Takes `&mut Scratch` |

Dropping a `Scratch` runs `cleanup` and ignores its error. Removal uses
[`std.fs.remove_tree`](fs.md) with budgets of 100000 entries and depth 64;
beyond them cleanup reports `TestError.Fixture(FsError.Limit)` and leaves the
rest in place. A process that traps or is killed skips destructors, so its
scratch directory remains, except under `plenty test`, which gives each test
its own `TMPDIR` and removes it when the test ends.

## Test doubles

Code that reads the clock or moves bytes can take its source as a protocol
bound, so a test passes a double and a program passes the real thing. Calls are
resolved at compile time; a double costs nothing in the program that does not
use it.

`fake_clock(nanoseconds_after_origin: u64)` returns a `FakeClock`, a
[`std.time.Clock`](time.md#clocks) whose time moves only when the test moves it.
Its instants are on the [`instant_at`](time.md#instants-and-deadlines) timeline.

| Method | Result | Contract |
|---|---|---|
| `now()` | `Instant` | The current fake instant |
| `advance(span: &Duration)` | `Result[(), RangeError]` | Move forward by `span`; past the end of the timeline is `RangeError.OutOfRange` and nothing moves. Takes `&mut FakeClock` |
| `set(nanoseconds_after_origin: u64)` | `()` | Move to any instant, including backwards. Takes `&mut FakeClock` |

`fake_stream(input: Bytes)` returns `Result[FakeStream, BytesError]`, a
[`std.io`](io.md#stream-contracts) `Reader` and `Writer` over memory. Reads
return `input` from its start; writes append to `output`. Both fields are
public. It injects the partial progress and failures that a file or socket
produces only under load:

| Method | Result | Contract |
|---|---|---|
| `limit(bytes: i64)` | `()` | Each later `read` or `write` moves at most `bytes` bytes; 0 or less removes the limit |
| `fail_after(after: i64, error: StreamError)` | `()` | The call `after` calls from now (0 is the next) returns `Err(error)` and moves nothing. One failure is pending at a time; a later request replaces it |
| `calls()` | `i64` | `read`, `write`, and `flush` calls made so far, including failed ones |
| `read`, `write`, `flush` | as `std.io` | The stream protocol methods |

A range outside its buffer is `StreamError.OutOfRange` before the call is
counted. `flush` does nothing but count and fail when told. Writing allocates
as `output` grows; that failure is `StreamError.Allocation`.

```text
from std.time import Clock, Instant

def overdue[C: Clock](clock: &C, since: &Instant) -> bool:
    clock.now().saturating_duration_since(since).as_seconds() > 30

def test_overdue() -> Result[(), Failure]:
    mut clock = std.test.fake_clock(0u64)
    since = clock.now()
    std.test.check(not overdue(&clock, &since), "fresh")?
    clock.advance(&std.time.seconds(31u32))?
    std.test.check(overdue(&clock, &since), "after 31s")?
    Ok(())
```

A program calls the same function with `std.time.monotonic_clock()`.

## Errors

| Variant | Meaning |
|---|---|
| `Assertion` | An assertion failed; its explanation is already on standard error |
| `Fixture(FsError)` | Creating or removing a scratch directory failed, or a name or path was invalid |
| `Allocation(AllocError)` | Memory for a fixture ran out |
| `InvalidSeed` | `PLENTY_TEST_SEED` is set but is not a decimal `u64` |

## Limits

- Assertions do not report the caller's source location.
- There is no allocation failure injection. Native I/O failures can be
  injected only at the stream boundary, through `FakeStream`; a `File`
  operation cannot be made to fail.
- `is_err` rejects a `Result[(), E]`, and `is_ok` a `Result[T, ()]`: their
  failure explanation borrows the unit value to show it, and unit cannot be
  borrowed. Match on the result instead.
