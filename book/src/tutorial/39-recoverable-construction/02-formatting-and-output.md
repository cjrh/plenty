# Formatting and output

`str.repr` formats a borrowed value; `print` writes a value and newline,
reporting formatting allocation failures and output errors:

```plenty
def main() -> Result[(), Failure]:
    values = [1, 2]?
    print(str.repr(values))?
    print(print(values))?
    Ok(())
```
```output
Result[str, AllocError].Ok("[1, 2]")
[1, 2]
Result[(), IoError].Ok(())
```

Formatting failure writes nothing. An output error can leave a partial write.
The source remains usable; neither call copies a mutable collection.
