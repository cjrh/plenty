# std.test

`import std.test` provides assertions, fixtures, and test doubles for the
tests that [`plenty test`](../toolchain/01-building/packages.md#tests) runs,
with one error type, `TestError`. The module is checked Plenty code. Any
program may import it; it depends on the runner only for reading
`PLENTY_TEST_SEED` and for [failure injection](#failure-injection).

A test file with `main` passes when it exits with status zero, so a plain
program can run assertions:

```plenty
import std.test
from std.test import TestError

def test_split() -> Result[(), AllocError | TestError]:
    parts = "a,b,,c".split(",")?
    std.test.equal(&len(parts), &4)?
    std.test.equal(&parts[2], &"")?
    std.test.check(parts[0] < parts[1], "parts keep their order")?
    Ok(())

def main() -> Result[(), AllocError | IoError | TestError]:
    test_split()?
    print("test_split passed")?
    Ok(())
```
```output
test_split passed
```

## Assertions

An assertion that holds returns `Ok(())` without allocating. One that fails
flushes standard output, writes the position of the call, `assertion failed: `,
and an explanation to standard error, and returns `Err(TestError.Assertion)`.
The explanation is written at once, so it survives propagation or handling by
the caller. Tests can return `TestError` or a sum containing it.

```text
parse.plenty:12:5: assertion failed: left == right
  left: 4
 right: 5
```

The file is relative to its source root, `tests/` for a test file, as in
[trap reports](../language/08-errors/02-allocation-and-traps.md#trap-reports).
The position is the user code that
called the assertion, even through
standard-library helpers: the
[caller location](../language/08-errors/02-allocation-and-traps.md#caller-locations)
that trap notes use. A user helper that wraps an assertion reports the helper's
line, not its caller's.

| Function | Passes when | Failure explanation |
|---|---|---|
| `fail(message: &str)` | never | `message` |
| `check(condition: bool, message: &str)` | `condition` | `message` |
| `equal[T](left: &T, right: &T)` | `left == right` | `left == right`, then `  left: ` and ` right: ` lines |
| `not_equal[T](left: &T, right: &T)` | `left != right` | `left != right`, then a `  both: ` line |
| `less[T](left: &T, right: &T)` | `left < right` | `left < right`, then `left` and `right` lines |
| `close(left: f64, right: f64, tolerance: f64)` | `left == right` or `abs(left - right) <= tolerance` | the three values; NaN on either side fails, equal infinities pass |
| `is_ok[T, E](result: &Result[T, E])` | `result` is `Ok` | `result is Ok`, then an `  error: ` line |
| `is_err[T, E](result: &Result[T, E])` | `result` is `Err` | `result is Err`, then a `  value: ` line |

Values are shown with `str.repr`, so `T` must support equality or ordering as
the function needs and have a `repr`. Showing a value allocates; when that
fails the line reads `<no memory to show the value>`. A failure to write to
standard error is ignored.

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

def test_overdue() -> Result[(), std.test.TestError | RangeError]:
    mut clock = std.test.fake_clock(0u64)
    since = clock.now()
    std.test.check(not overdue(&clock, &since), "fresh")?
    clock.advance(&std.time.seconds(31u32))?
    std.test.check(overdue(&clock, &since), "after 31s")?
    Ok(())
```

A program calls the same function with `std.time.monotonic_clock()`.

## Failure injection

Executables that `plenty test` builds can make the C library's allocation and
file calls fail, so a test reaches the error paths of code that allocates or
does file I/O. Injection belongs to the calling thread.

| Function | Effect |
|---|---|
| `fail_allocations_after(count: u32)` | After `count` more allocations succeed, each fails until restored: the operation reports `AllocError.OutOfMemory` |
| `restore_allocations()` | Allocations succeed again |
| `fail_files_after(count: u32, code: i32)` | After `count` more file calls succeed, each fails with errno `code` until restored: the operation reports `IoError.System(code)` |
| `restore_files()` | File calls succeed again |

All four return `Result[(), TestError]`. A later call replaces the budget.
Counted file calls are opening any path, and reading, writing, syncing, or
truncating a regular file; standard streams, pipes, and sockets never fail, so
a failing test can still report. Other filesystem calls, such as renaming or
listing a directory, are not counted. How many calls an operation makes is not
part of its contract, so a test should fail from the first call (`count` 0) or
check only that some call failed. A failure that `?` propagates leaves the
budget in place; restore it before an assertion that must allocate to show its
values.

```text
def test_save_reports_a_full_disk() -> Result[(), std.test.TestError]:
    scratch = std.test.scratch()?
    name = scratch.file("state.txt")?
    std.test.fail_files_after(0u32, 28i32)?
    saved = write_text(&name, "state")
    std.test.restore_files()?
    full: Result[i64, IoError] = Err(IoError.System(28i32))
    std.test.equal(&saved, &full)?
    Ok(())
```

In any other executable, including one built by `plenty build` or `plenty
run`, these functions write `error: failure injection needs an executable
built by plenty test` to standard error and return
`Err(TestError.Unavailable)`. Only test builds link the interposition code;
other executables do not contain it.

## Errors

| Variant | Meaning |
|---|---|
| `Assertion` | An assertion failed; its explanation is already on standard error |
| `Fixture(FsError)` | Creating or removing a scratch directory failed, or a name or path was invalid |
| `Allocation(AllocError)` | Memory for a fixture ran out |
| `InvalidSeed` | `PLENTY_TEST_SEED` is set but is not a decimal `u64` |
| `Unavailable` | Failure injection was requested outside a `plenty test` build |

## Limits

- Injected failures cover allocation and the file calls listed above, on the
  calling thread only. Allocation on other threads, such as executor workers,
  and network calls are not injected; use `FakeStream` at the stream boundary.
- Calls the C library makes internally, such as `fopen` inside another C
  function, are not intercepted; only the program's and runtime's own calls
  are.
