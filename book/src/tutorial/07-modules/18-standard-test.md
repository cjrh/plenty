# Write tests with std.test

`plenty test` runs a package's tests. A file in `tests/` holds either one
program with `main`, as in [the packages lesson](16-packages.md), or any number
of `test_` functions. Each test function runs in its own process, so a trap in
one fails only that test.

Here is a small library to test:

```toml project-file stats/plenty.toml
manifest-version = 1

[package]
name = "stats"
version = "0.1.0"
```

```plenty project-file stats/src/lib.plenty
from std.bytes import from_text
from std.io import Writer, Partial, write_all
from std.bytes import BytesError

pub def total(values: &list[i64]) -> i64:
    mut sum = 0
    for value in values:
        sum += *value
    sum

pub def mean(values: &list[i64]) -> f64:
    f64(total(values)) / f64(len(values))

pub def save(path: &str, values: &list[i64]) -> Result[(), AllocError | IoError]:
    write_text(path, &str.from(total(values))?)?
    Ok(())

pub def report[W: Writer](out: &mut W, values: &list[i64]) -> Result[(), AllocError | BytesError | Partial]:
    line = from_text(&str.from(total(values))?)?
    write_all(out, &line, 0, line.len())?
    Ok(())
```

A test function takes no parameters and returns `()` or `Result[(), E]`.
`std.test` supplies assertions that return `Result[(), TestError]`, so `?`
stops the test at the first failure:

```plenty project-file stats/tests/sums.plenty
import std.bytes
import std.test
import std.io
from stats import mean, report, save, total

def test_total() -> Result[(), AllocError | IoError | std.test.TestError]:
    values = [1, 2, 3]?
    std.test.equal(&total(&values), &6)?
    empty: list[i64] = []?
    std.test.equal(&total(&empty), &0)?
    Ok(())

def test_mean() -> Result[(), AllocError | IoError | std.test.TestError]:
    values = [1, 2]?
    std.test.close(mean(&values), 1.5, 0.000001)?
    Ok(())

def test_save() -> Result[(), AllocError | IoError | std.test.TestError]:
    scratch = std.test.scratch()?
    path = scratch.file("total.txt")?
    values = [4, 5]?
    save(&path, &values)?
    std.test.equal(&read_text(&path)?, &"9")?
    Ok(())

def test_order_does_not_matter() -> Result[(), AllocError | IoError | std.test.TestError]:
    mut rng = std.test.rng()?
    mut values = [1, 2, 3, 4, 5]?
    rng.shuffle(&mut values)
    std.test.equal(&total(&values), &15)?
    Ok(())

def test_report_survives_short_writes() -> Result[(), AllocError | std.bytes.BytesError | std.io.Partial | std.test.TestError]:
    mut out = std.test.fake_stream(std.bytes.empty()?)?
    out.limit(1)
    values = [9, 10]?
    report(&mut out, &values)?
    std.test.equal(&out.output.to_text()?, &"19")?
    Ok(())

def test_save_reports_a_full_disk() -> Result[(), AllocError | IoError | std.test.TestError]:
    scratch = std.test.scratch()?
    path = scratch.file("total.txt")?
    values = [4, 5]?
    std.test.fail_files_after(0u32, 28i32)?
    saved = save(&path, &values)
    std.test.restore_files()?
    std.test.is_err(&saved)?
    Ok(())
```

`std.test.scratch()` makes an empty directory under `$TMPDIR` that is removed
with everything in it when the `Scratch` value is dropped. `std.test.rng()` is
a generator seeded from `plenty test --seed N`, or from 0, so a run repeats
exactly.

`report` takes any `std.io` `Writer`, so the test hands it a fake stream that
accepts one byte per call. A file rarely takes a write in pieces, but a pipe or
socket does; the fake makes that case happen every time. `fail_after` makes a
chosen call fail, and `std.test.fake_clock` stands in for the clock in the same
way.

A real file fails rarely, so `test_save_reports_a_full_disk` makes it fail:
after `fail_files_after(0u32, 28i32)`, every file call fails with error 28, the
system's code for a full disk, until `restore_files`. `fail_allocations_after`
does the same for memory. Both work only in the executables `plenty test`
builds; a shipped program cannot be made to fail this way.

```sh plenty-project stats
plenty test
```

```output
running 6 tests
test sums::test_total ... ok
test sums::test_mean ... ok
test sums::test_save ... ok
test sums::test_order_does_not_matter ... ok
test sums::test_report_survives_short_writes ... ok
test sums::test_save_reports_a_full_disk ... ok

test result: ok. 6 passed; 0 failed
```

Tests run in parallel, one per CPU, but are listed in the order they appear.

Each test is named `FILE::FUNCTION`. `plenty test mean` runs only the tests
whose names contain `mean`, and `plenty test --list` prints the names without
running anything.

A test's output is kept and shown only if it fails. If `total` added one too
many, `plenty test test_total` would print:

```text
running 1 test
test sums::test_total ... FAILED (returned Err)

failures:

---- sums::test_total output ----
sums.plenty:7:5: assertion failed: left == right
  left: 7
 right: 6

test result: FAILED. 0 passed; 1 failed; 5 filtered out
```

The command exits with status 1.

The assertion writes where it was called and what it found as it fails, and
`returned Err` says the test stopped there. A trap would read `exit status 1` and show the trap's
message. `plenty test --no-capture` lets output through as it happens, and
`--timeout 10` fails any test still running after ten seconds.

A file defines either `main` or `test_` functions, not both. Helpers that are
not tests need a name without the `test_` prefix, or a module in a subdirectory
of `tests/`. See [`std.test`](../../design/stdlib/test.md) for every assertion.
