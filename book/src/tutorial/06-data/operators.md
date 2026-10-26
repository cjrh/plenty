# Give arithmetic meaning to your types

Use arithmetic operators when their meaning is natural for your type. A method
named `__add__` defines `+`, and `__neg__` defines unary `-`. These are statically
compiled calls, so using the operator does not introduce runtime method lookup.

```plenty
class Distance:
    metres: i64

    def __add__(self, other: &Distance) -> Distance:
        Distance(self.metres + other.metres)

    def __neg__(self) -> Distance:
        Distance(-self.metres)

    def __mul__(self, scale: i64) -> Distance:
        Distance(self.metres * scale)

    def __rmul__(self, scale: i64) -> Distance:
        self * scale

    def __iadd__(self: &mut Distance, other: &Distance) -> ():
        self.metres += other.metres

def main() -> Result[(), IoError]:
    mut walk = Distance(10)
    extra = Distance(5)
    print(walk + extra, -extra, 3 * extra)?
    walk += extra
    print(walk, extra)?
    Ok(())
```
```output
Distance(metres=15) Distance(metres=-5) Distance(metres=15)
Distance(metres=15) Distance(metres=5)
```

The ordinary hooks borrow their operands, so `extra` stays usable. The reflected
hook `__rmul__` handles a distance on the right of `3 * extra`. `__iadd__` takes
exclusive access and updates `walk` in place. It returns `()`, and the binding
must be mutable.

For operations that may fail, return `Result` and handle it at the call site.
Here a domain check prevents negative combined quantities:

```plenty
class Quantity:
    amount: i64

    def __add__(self, other: &Quantity) -> Result[Quantity, str]:
        total = self.amount + other.amount
        if total < 0:
            return Err("negative quantity")
        Ok(Quantity(total))

def main() -> Result[(), IoError | str]:
    a = Quantity(5)
    b = Quantity(3)
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

Equality still compares fields automatically; declaring `__eq__` does not change
it. For supported hooks, exact signatures, and resolution rules, see
[user-defined arithmetic operators](../../design/language/04-expressions/operators.md).
