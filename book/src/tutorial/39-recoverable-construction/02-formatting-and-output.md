# Formatting and output

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

Both calls can fail. The inner `?` handles formatting; the outer `?` handles
printing. Forgetting either is a compile error. To inspect a Result wrapper
deliberately, pass it to `str.repr`:

```plenty
def main() -> Result[(), Failure]:
    result: Result[i64, str] = Err("not ready")
    print(str.repr(result)?)?
    Ok(())
```
```output
Result[i64, str].Err("not ready")
```
