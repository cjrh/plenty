# std.test

`import std.test` provides assertions and fixtures for the tests that
[`plenty test`](../toolchain/01-building/packages.md#tests) runs, with one
error type, `TestError`. The module is checked Plenty code with no native
primitives. Any program may import it; it does not depend on the runner except
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
scratch directory remains.

## Errors

| Variant | Meaning |
|---|---|
| `Assertion` | An assertion failed; its explanation is already on standard error |
| `Fixture(FsError)` | Creating or removing a scratch directory failed, or a name or path was invalid |
| `Allocation(AllocError)` | Memory for a fixture ran out |
| `InvalidSeed` | `PLENTY_TEST_SEED` is set but is not a decimal `u64` |

## Limits

There is no failure injection, fake clock, or fake stream in this module. Use
[`std.io`](io.md) cursors for in-memory streams and `std.time.instant_at` for
fixed instants.
