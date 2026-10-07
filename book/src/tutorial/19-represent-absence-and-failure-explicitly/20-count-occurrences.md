# Count occurrences

`text.count(needle)` returns an `i64` without allocating. Matches are literal
and do not overlap. An empty needle counts Unicode scalar boundaries:

```plenty
def main() -> Result[(), IoError]:
    print("banana".count("ana"))?
    print("aaaaa".count("aa"))?
    print("é🙂".count(""))?
    print("".count(""))?
    print("text".count("missing"))?
    Ok(())
```

```output
1
2
3
1
0
```

The method borrows both strings and takes exactly one pattern argument.
