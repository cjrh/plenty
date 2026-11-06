# Supply a default or a typed error

`option.unwrap_or(default)` returns a present payload or the supplied default.
`dictionary.get(key, default)` does the same for a dictionary lookup, while
`dictionary.get(key)` still returns an `Option`. Zero, `False`, and an empty
string count as present values.

```plenty
def main() -> Result[(), AllocError | IoError]:
    numbers = {"answer": 42}?
    print(numbers.get("answer", 0))?
    print(numbers.get("missing", 0))?
    missing: Option[i64] = Nothing
    print(missing.unwrap_or(9))?
    Ok(())
```

```output
42
0
9
```

`option.ok_or(error)` converts absence into a typed `Result`. Choose an error
that explains the missing value, then use `?` inside a function returning a
compatible `Result`.

```plenty
enum Missing:
    Answer

def answer(numbers: &dict[str, i64]) -> Result[i64, Missing]:
    value = numbers.get("answer").ok_or(Missing.Answer)?
    Ok(value)

def main() -> Result[(), AllocError | IoError]:
    numbers = {"answer": 42}?
    match answer(&numbers):
        case Ok(value):
            print(value)?
        case Err(_):
            print("missing answer")?
    Ok(())
```

```output
42
```

These helpers add no allocation. They evaluate their arguments eagerly, even
when a value is present. The Option methods consume owned payloads and drop an
unused default or error. Dictionary lookup supports copyable values; use
`pop` to remove and take an owned value. Use `match` when the fallback should
run only on absence, or choose a lazy helper as described below. Bare `Nothing` may need an Option annotation or a typed
result context to determine its payload type.

## Compute a default only when needed

Both `Option` and `Result` provide `unwrap_or_else`: it calls a fallback only
for `Nothing` or `Err`. A Result fallback receives the error. Option also has
`ok_or_else` for constructing an absence error lazily. These take function
values; [functions as values](../../design/language/06-functions/02-function-values.md)
and [closures](../../design/language/06-functions/03-closures.md) explain
callbacks in more detail.

A fallback can itself fail. In that case, `map_or_else` lets both branches
return a Result, and `?` handles that Result after selection:

```plenty
def default_port() -> Result[i64, ParseError]:
    i64.parse("8080")

def port(configured: Option[i64]) -> Result[i64, ParseError]:
    Ok(configured.map_or_else(default_port, Ok)?)

def main() -> Result[(), ParseError | IoError]:
    print(port(Some(9000))?, port(Nothing)?)?
    Ok(())
```
```output
9000 8080
```

`default_port` runs only for `Nothing`. Writing
`configured.unwrap_or(default_port()?)` would run it even for `Some(9000)`.
A `match` is equally suitable when either branch needs several statements.
`Result.unwrap_or(default)` also exists for deliberate recovery with a cheap,
already available value. See [defaults and recovery](../../design/language/08-errors/01-sums-and-propagation.md#defaults-and-recovery)
for ownership, evaluation order, and the Result callback's error argument.
