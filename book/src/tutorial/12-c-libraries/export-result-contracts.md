# Report errors through an exported function

Exports can return a numeric error through `Result`. Plenty callers use the
ordinary result; C callers receive a status and separate output pointers:

```plenty
export def positive(value: i32) -> Result[i32, i32] = "calc_positive":
    if value < 0:
        Err(1)
    else:
        Ok(value)

def main() -> Result[(), Failure]:
    print(positive(42)?)?
    print(str.repr(positive(-1))?)?
    Ok(())
```
```output
42
Result[i32, i32].Err(1)
```

The generated C declaration is `uint32_t calc_positive(int32_t p0,
int32_t *out_ok, int32_t *out_error)`. Status 0 writes `out_ok`; status 1 writes
`out_error`. Its header explains output storage and borrowing requirements.
