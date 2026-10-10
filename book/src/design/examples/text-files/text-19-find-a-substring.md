# Find a substring

`find` and `rfind` return the first or last match as `Option[i64]`. Unlike Python's
`-1` sentinel, a missing match is `Nothing`. Positions count Unicode scalars:

```plenty
def main() -> Result[(), IoError]:
    text = "é🙂é🙂"
    print(text.find("🙂"))?
    print(text.rfind("🙂"))?
    print(text.find("missing"))?
    print(text.rfind(""))?
    Ok(())
```

```output
Option[i64].Some(1)
Option[i64].Some(3)
Option[i64].Nothing
Option[i64].Some(4)
```

Both operations borrow their inputs and allocate nothing. Use `match` or `?` in
an `Option`-returning function. An empty needle matches at the beginning for
`find` and at the end for `rfind`. Each takes one literal pattern; optional
bounds and regexes are not supported.
