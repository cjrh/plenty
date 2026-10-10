# Repeat text

`text.repeat(count)` accepts an `i64` and returns `Result[str, AllocError]`.
Zero and negative counts produce an empty string:

```plenty
def main() -> Result[(), Failure]:
    print(str.repr("é🙂".repeat(3))?)?
    print(str.repr("x".repeat(0))?)?
    print(str.repr("x".repeat(-2))?)?
    print(str.repr("x".repeat(9223372036854775807))?)?
    Ok(())
```
```output
Result[str, AllocError].Ok("é🙂é🙂é🙂")
Result[str, AllocError].Ok("")
Result[str, AllocError].Ok("")
Result[str, AllocError].Err(AllocError.CapacityOverflow)
```

The source stays usable. The runtime checks the complete size before allocating
one independent output string; even empty and single-copy outputs can fail to
allocate. Empty input with a large count is handled directly. String multiplication
syntax is not supported yet.
