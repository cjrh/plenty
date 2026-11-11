# Keep a numeric bound in the type

A configuration value often needs a fixed interval. Name that interval with
`Bounded`, then validate computed input once before passing it around:

```plenty
type RetryCount = Bounded[i64, 0, 8]

def describe(retries: RetryCount) -> Result[(), IoError]:
    print("Retries:", retries.get())

def main() -> Result[(), RangeError | IoError]:
    describe(3)?
    configured = 5
    retries = RetryCount.try_from(configured)?
    describe(retries)?
    print(RetryCount.try_from(9).is_err())?
    Ok(())
```
```output
Retries: 3
Retries: 5
True
```

Both ends are included. The literal `3` is checked at compile time because the
parameter expects `RetryCount`. A computed integer goes through `try_from`,
which returns `RangeError.OutOfRange` when it does not fit. `.get()` extracts the
integer for calculations. A literal `9` passed directly to `describe` would be
a compile error.

## Convert a value the compiler can bound

Some expressions have a range that follows from their form. A remainder by a
positive constant `c` is in `0..c - 1`, whatever its left operand. `from`
accepts such a value without a run-time check and without a `Result`:

```plenty
const SLOTS: i64 = 16
const LAST_SLOT: i64 = SLOTS - 1
type Slot = Bounded[i64, 0, LAST_SLOT]

def slot(key: i64) -> Slot:
    Slot.from(key % SLOTS)

def main() -> Result[(), IoError]:
    print(slot(37).get(), slot(-1).get())?
    Ok(())
```
```output
5 15
```

`from` also accepts `x & c` with a constant `c` of at least zero, and another
`Bounded` whose interval lies inside the target. For anything else, including
`key % SLOTS + 1`, it is a compile error that points to `try_from`: there is no
conversion that skips the check on trust.

```plenty-error
type Slot = Bounded[i64, 0, 15]

def slot(key: i64) -> Slot:
    Slot.from(key)

def main() -> ():
    slot(37)
```
```error
use `Bounded.try_from(value)?` to check a value at run time
```

Prefer `from` when it applies, `try_from` at input boundaries, and a literal
for a fixed value. The
[bounded integer reference](../../design/language/03-types/bounded.md#proved-ranges)
lists the accepted forms.

## Abstract over the bounds

The arguments `0` and `8` are compile-time values, not types. A generic function
can keep them as `const` parameters:

```plenty
def interval[T: IntType, const lo: T, const hi: T](value: &Bounded[T, lo, hi]) -> (T, T):
    (lo, hi)

def main() -> Result[(), IoError]:
    retries: Bounded[i64, 0, 8] = 3
    percentage: Bounded[u8, 0, 100] = 75
    print(interval(&retries), interval(&percentage))?
    Ok(())
```
```output
(0, 8) (0, 100)
```

The argument's type supplies `T`, `lo` and `hi`; no runtime integer becomes a
specialization argument. `const lo: T` uses the earlier `T: IntType`, so a `u8`
interval has `u8` bounds. The values are available inside the body without
storing them alongside each integer.

User classes, enums and methods can also declare const parameters. Use them
when the value belongs in type identity. A setting read from a file normally
stays a runtime value: it cannot select a new type at runtime. The
[value-parameter reference](../../design/language/07-generics/values.md) gives
argument restrictions, and [bounded integers](../../design/language/03-types/bounded.md)
describes conversion and value behavior.
