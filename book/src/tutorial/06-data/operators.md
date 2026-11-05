# Give arithmetic meaning to your types

Use arithmetic operators when their meaning is natural for your type. A method
named `__add__` defines `+`, and `__neg__` defines unary `-`. These are statically
compiled calls, so using the operator does not introduce runtime method lookup.

The distance wrappers here demonstrate hook and forwarding mechanics. For physical
calculations, use [quantities](../09-generics/dimensions.md): they also check the
dimensions of products and quotients, which these wrappers do not.

```plenty
class Distance:
    metres: i64

    def __add__(self, other: &Distance) -> Result[Distance, OverflowError[i64]]:
        Ok(Distance(self.metres + other.metres))

    def __neg__(self) -> Result[Distance, OverflowError[i64]]:
        Ok(Distance(-self.metres))

    def __mul__(self, scale: i64) -> Result[Distance, OverflowError[i64]]:
        Ok(Distance(self.metres * scale))

    def __rmul__(self, scale: i64) -> Result[Distance, OverflowError[i64]]:
        self * scale

    def add(self: &mut Distance, other: &Distance) -> Result[(), OverflowError[i64]]:
        self.metres += other.metres
        Ok(())

def main() -> Result[(), IoError | OverflowError[i64]]:
    mut walk = Distance(10)
    extra = Distance(5)
    print((walk + extra)?, (-extra)?, (3 * extra)?)?
    walk.add(&extra)?
    print(walk, extra)?
    Ok(())
```
```output
Distance(metres=15) Distance(metres=-5) Distance(metres=15)
Distance(metres=15) Distance(metres=5)
```

The ordinary hooks borrow their operands, so `extra` stays usable. The reflected
hook `__rmul__` handles a distance on the right of `3 * extra`. The named `add`
method takes exclusive access and updates `walk` in place. Its integer addition
can fail, so it returns a `Result`; the binding must be mutable.

Integer hooks declare overflow just like ordinary functions. A hook can also
report a domain error; here the extra check prevents negative combined stock:

```plenty
class Stock:
    amount: i64

    def __add__(self, other: &Stock) -> Result[Stock, str | OverflowError[i64]]:
        total = self.amount + other.amount
        if total < 0:
            return Err("negative stock")
        Ok(Stock(total))

def main() -> Result[(), IoError | str | OverflowError[i64]]:
    a = Stock(5)
    b = Stock(3)
    combined = (a + b)?
    print(combined.amount)?
    Ok(())
```
```output
8
```

Parenthesize the operation before `?`. Allocation errors follow the same pattern.
For a fallible update, write `value = (value + other)?` or call a named method;
`+=` requires a unit-returning in-place hook. For large data, a named method that
writes into reusable output storage can avoid intermediate allocations.

A class that only gives a distinct name to one value can forward its field's
operators instead of defining a hook for each. Annotate it `@newtype`:

```plenty
@newtype
class Metres:
    value: i64

@newtype
class Seconds:
    value: i64

def main() -> Result[(), IoError | OverflowError[i64]]:
    run = Metres(400)
    mut total = run + run
    total += Metres(100)
    print(total, total > run)?
    lap = Seconds(70)
    print(lap * Seconds(2))?
    Ok(())
```
```output
Metres(value=900) True
Seconds(value=140)
```

`run + run` adds the two `value` fields and wraps the sum back in `Metres`, and
comparisons such as `>` return `bool`. Typing stays nominal: `run + lap` and
`run + 100` are compile errors, which keeps metres and seconds apart. An
explicit hook such as `__add__` takes over its own operator. `@newtype` does
not make the class copy; add `@copy` for that.

Equality still compares fields automatically. Python special methods that
Plenty never calls, such as `__eq__`, `__hash__`, `__str__` and `__len__`, are
compile errors rather than silently ignored; see
[equality, ordering and hashing](../../design/language/04-expressions/operators.md#equality-ordering-and-hashing)
and [reserved method names](../../design/language/04-expressions/operators.md#reserved-method-names).
For supported hooks, exact signatures, and resolution rules, see
[user-defined arithmetic operators](../../design/language/04-expressions/operators.md).
