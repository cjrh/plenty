# Calculate with numbers

Arithmetic requires matching types:

```plenty-error
def main() -> ():
    small = 7u8
    print(small + 1i64).unwrap()
```

```error
expected u8, got i64
```

Choose a matching literal or explicitly convert a value:

```plenty
def main() -> Result[(), IoError]:
    small = 7u8
    print(small + 1u8)?
    print(i64(small) + 1)?
    Ok(())
```

```output
8
8
```

Integer arithmetic overflow is a runtime error. A cast can still discard data:
narrowing discards high bits, so `u8(257)` produces `1`. Widening sign-extends
signed inputs and zero-extends unsigned inputs; same-width signed/unsigned casts
reinterpret the bits.
Casts do not validate a value's range.

`+`, `-`, and `*` have their usual arithmetic precedence. `//` divides integers
and rounds down, including for negative values:

```plenty
def main() -> Result[(), IoError]:
    print(1 + 2 * 3)?
    print((1 + 2) * 3)?
    print(-7 // 3)?
    Ok(())
```

```output
7
9
-3
```

Integer division by zero is a runtime error. Use `/` for floating-point division.
