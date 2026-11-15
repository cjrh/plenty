# Calculate with large integers

Fixed-width integer arithmetic reports overflow. When the value itself must
grow, use [`std.bigint`](../../design/stdlib/bigint.md). Its operators borrow
their operands, return owned results, and report allocation failure explicitly.

```plenty
from std.bigint import from_i64
from std.fmt import FormatError

def main() -> Result[(), AllocError | IoError | FormatError]:
    mut factorial = from_i64(1)
    for n in range(2, 26):
        factorial = (factorial * from_i64(n))?
    print(factorial)?
    print(factorial.to_i64().is_err())?
    Ok(())
```
```output
15511210043330985984000000
True
```

`from_i64` does not allocate. An arithmetic result outside the inline range
uses fallible limb storage; the `?` after multiplication propagates failure.
There is no `*=` fallback because an in-place hook cannot return that failure.
Both operands remain available after an arithmetic expression.

Use `parse(text, radix=10, max_digits=10000)` for text and
`value.to_string(radix=10, max_digits=10000)` for owned output. These digit caps
exclude a leading sign. Ordinary display uses the same 10,000-digit cap.
Division and remainder use floor semantics, including for negative operands.
Use `checked_divmod` when a zero divisor should be a recoverable error, and
`cmp` when you need ordering. Exact conversion back to a fixed-width integer
can fail with `BigIntError.OutOfRange`.
