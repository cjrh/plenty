# Python to Plenty

Read the skill's cheat sheet and signature index before writing the port.
Similar indentation does not imply the same types, ownership or failure model.

## A practical sequence

1. Record the Python behavior, including mutation/aliasing, integer ranges,
   missing values, exceptions, text versus bytes and output/exit status.
2. Give each parameter and result a concrete type. Use `i64` only when the
   domain fits; use [std.bigint](../../../../book/src/design/stdlib/bigint.md)
   when arbitrary precision is part of the contract. Represent heterogeneous
   values with an enum or `A | B`; do not erase distinctions to make it compile.
3. Decide who owns each collection. Read through `&list[T]` or `&[T]`, mutate
   through `&mut`, and take a plain `T` when consuming it. Rebind with `mut` only
   when updates are needed. Reproduce shared mutation explicitly rather than
   adding copies until borrow errors disappear.
4. Declare the actual error union and propagate with `?`, or recover with
   `match`. Test the port against the Python version on valid inputs and
   expected failures. Run both `plenty --check FILE` and `plenty FILE`.

## Common replacements

| Python | Plenty |
|---|---|
| Module-level execution | Put executable statements in `main`. |
| `None`, `Optional[T]` | `Nothing`, `Option[T]`; unwrap with `match`, or `.ok_or(error)?` in a `Result` function. |
| Exceptions | `Result[T, E]`; `?` propagates, `match` recovers. An `Err` from `main` reports failure and exits with status 1. |
| Implicit list/dict/string allocation | Handle literal construction, `append`, `insert`, copies, owned slices and f-strings with `?`. `print` also returns a `Result`. |
| `b = a` for a list | Moves the owner. Borrow it, or use `copy(a)?` if two independent owners are required. `str` and `bytes` values copy implicitly. |
| `d[k] = value` | `d.insert(k, value)?`; for mutation use `d.get_mut(k)` or `get_or_insert`. |
| `if xs`, `a < b < c`, `x ** n` | `if len(xs) > 0`, `a < b and b < c`, `x.pow(n)`. |
| `lambda` / `map` / `filter` | Typed inline `def(...) -> T: ...` and `iter(source).map(f).filter(p)`, or a clear loop. |
| `xs[a:b]` | `xs[a:b]?` makes an owning copy; `&xs[a:b]` lends a view without allocation. |
| A name assigned inside a branch, read afterward | Use `name = if condition:` with a value in each suite, or declare a `mut` binding outside the branch. |

Fixed-width arithmetic may propagate `OverflowError[T]`; variable division and
shifts have additional contracts. Do not silence these by wrapping the port in
`unchecked:`: violations trap, they do not wrap. `T(value)` traps if it cannot
represent the value; `T.try_from(value)?` gives a recoverable `RangeError`.
Unlike Rust/C, Plenty signed `//` and `%` already have Python's floor semantics.
Strings use Unicode scalar positions, not UTF-8 byte offsets; use byte APIs for
binary protocols. Missing indexed elements trap; use `.get()` for recoverable
absence rather than assuming Python's `IndexError`/`KeyError` handling survives.

## Example: borrowed input, fallible accumulation

Python:

```python
def total(values: list[int]) -> int:
    result = 0
    for value in values:
        result += value
    return result
```

Plenty, choosing a checked `i64` domain explicitly:

```plenty
def total(values: &list[i64]) -> Result[i64, OverflowError[i64]]:
    mut result = 0
    for value in values:
        result += *value
    Ok(result)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    values = [1, -2, 3]?
    print(total(&values)?)?
    print(total(&values)?)?
    Ok(())
```
```output
2
2
```

Both calls print `2`; the input remains owned by `main`. If inputs can exceed
`i64`, this is a deliberate restricted port, not a faithful arbitrary-precision
replacement. For detailed errors and aliasing decisions, read
[errors](errors.md) and [ownership](ownership.md).
