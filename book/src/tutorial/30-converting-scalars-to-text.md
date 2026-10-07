# Converting scalars to text

`str.from` converts numbers or booleans with one recoverable string
allocation. It returns `Result[str, AllocError]`, so it combines with `?` and
the existing fallible string operations. Floats use shortest round-trip text.

```plenty
def main() -> Result[(), IoError]:
    print(str.from(42))?
    print(str.from(-0.0f32))?
    print(str.from(True))?
    Ok(())
```
```output
Result[str, AllocError].Ok("42")
Result[str, AllocError].Ok("-0.0")
Result[str, AllocError].Ok("True")
```
