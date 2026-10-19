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
run only on absence. Bare `Nothing` may need an Option annotation or a typed
result context to determine its payload type.
