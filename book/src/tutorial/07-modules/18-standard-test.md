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
pub def total(values: &list[i64]) -> i64:
    mut sum = 0
    for value in values:
        sum += *value
    sum

pub def mean(values: &list[i64]) -> f64:
    f64(total(values)) / f64(len(values))

pub def save(path: &str, values: &list[i64]) -> Result[(), Failure]:
    write_text(path, &str.from(total(values))?)?
    Ok(())
```

A test function takes no parameters and returns `()` or `Result[(), E]`.
`std.test` supplies assertions that return `Result[(), TestError]`, so `?`
stops the test at the first failure:

```plenty project-file stats/tests/sums.plenty
import std.test
from stats import mean, save, total

def test_total() -> Result[(), Failure]:
    values = [1, 2, 3]?
    std.test.equal(&total(&values), &6)?
    empty: list[i64] = []?
    std.test.equal(&total(&empty), &0)?
    Ok(())

def test_mean() -> Result[(), Failure]:
    values = [1, 2]?
    std.test.close(mean(&values), 1.5, 0.000001)?
    Ok(())

def test_save() -> Result[(), Failure]:
    scratch = std.test.scratch()?
    path = scratch.file("total.txt")?
    values = [4, 5]?
    save(&path, &values)?
    std.test.equal(&read_text(&path)?, &"9")?
    Ok(())

def test_order_does_not_matter() -> Result[(), Failure]:
    mut rng = std.test.rng()?
    mut values = [1, 2, 3, 4, 5]?
    rng.shuffle(&mut values)
    std.test.equal(&total(&values), &15)?
    Ok(())
```

`std.test.scratch()` makes an empty directory under `$TMPDIR` that is removed
with everything in it when the `Scratch` value is dropped. `std.test.rng()` is
a generator seeded from `plenty test --seed N`, or from 0, so a run repeats
exactly.

```sh plenty-project stats
plenty test
```

```output
running 4 tests
test sums::test_total ... ok
test sums::test_mean ... ok
test sums::test_save ... ok
test sums::test_order_does_not_matter ... ok

test result: ok. 4 passed; 0 failed
```

Each test is named `FILE::FUNCTION`. `plenty test mean` runs only the tests
whose names contain `mean`, and `plenty test --list` prints the names without
running anything.

A test's output is kept and shown only if it fails. If `total` added one too
many, `plenty test test_total` would print:

```text
running 1 test
test sums::test_total ... FAILED (exit status 1)

failures:

---- sums::test_total output ----
assertion failed: left == right
  left: 7
 right: 6
error: test_total returned Failure.Unspecified

test result: FAILED. 0 passed; 1 failed; 3 filtered out
```

The command exits with status 1.

The assertion writes its explanation as it fails; the last line is the error
that reached the end of the test. `plenty test --no-capture` lets output through
as it happens.

A file defines either `main` or `test_` functions, not both. Helpers that are
not tests need a name without the `test_` prefix, or a module in a subdirectory
of `tests/`. See [`std.test`](../../design/stdlib/test.md) for every assertion.
