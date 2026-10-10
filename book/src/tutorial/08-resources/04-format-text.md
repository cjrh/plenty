# Convert and format values

`str.from` converts numbers or booleans to text, returning
`Result[str, AllocError]`. Floats use the shortest text that parses back to the
same value.

```plenty
def main() -> Result[(), Failure]:
    print(str.from(42)?)?
    print(str.from(-0.0f32)?)?
    print(str.from(True)?)?
    Ok(())
```
```output
42
-0.0
True
```

## Inspect a value with str.repr

`str.repr` formats a borrowed value; `print` writes a value and newline,
reporting formatting allocation failures and output errors:

```plenty
def main() -> Result[(), Failure]:
    values = [1, 2]?
    print(str.repr(values)?)?
    print(values)?
    Ok(())
```
```output
[1, 2]
[1, 2]
```

Formatting failure writes nothing. An output error can leave a partial write.
The source remains usable; neither call copies a mutable collection.

The inner `?` handles formatting; the outer `?` handles printing. To inspect a
`Result` wrapper itself, pass it to `str.repr`:

```plenty
def main() -> Result[(), Failure]:
    result: Result[i64, str] = Err("not ready")
    print(str.repr(result)?)?
    Ok(())
```
```output
Result[i64, str].Err("not ready")
```
