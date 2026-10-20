# Report errors through an exported function

Exports can return a numeric error through `Result`. Plenty callers use the
ordinary result; C callers receive a status and separate output pointers:

```plenty
export def positive(value: i32) -> Result[i32, i32] = "calc_positive":
    if value < 0:
        Err(1)
    else:
        Ok(value)

def main() -> Result[(), AllocError | IoError | i32]:
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

## Name errors with an enum

A numeric code means little to a C caller. Return an enum whose variants have
no payloads instead:

```plenty
enum CalcError:
    Empty
    Negative

export def checked(value: i32) -> Result[i32, CalcError] = "calc_checked":
    if value == 0:
        Err(CalcError.Empty)
    elif value < 0:
        Err(CalcError.Negative)
    else:
        Ok(value)

def main() -> Result[(), IoError | CalcError]:
    print(checked(42)?)?
    print(str.repr(checked(-1)).unwrap())?
    Ok(())
```
```output
42
Result[i32, CalcError].Err(CalcError.Negative)
```

C receives a `uint32_t` code, the variant's position in the declaration. The
header names each code:

```c
#define calc_CalcError_Empty UINT32_C(0)
#define calc_CalcError_Negative UINT32_C(1)
uint32_t calc_checked(int32_t p0, int32_t *out_ok, uint32_t *out_error);
```

The generated `calc.plentyi` declares its own `CalcError`, so Plenty consumers
match `calc.CalcError` variants. Enums with payloads and sums such as
`AllocError | CalcError` cannot cross the C boundary.
