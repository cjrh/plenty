# Check a text prefix or suffix

`startswith` and `endswith` borrow strings and return `bool` without allocating.
Matches are literal and case-sensitive; an empty prefix or suffix always matches.

```plenty
def main() -> Result[(), IoError]:
    name = "report.plenty"
    print(name.startswith("report"))?
    print(name.endswith(".plenty"))?
    print(name.endswith(".PLENTY"))?
    print("".startswith(""))?
    Ok(())
```

```output
True
True
False
True
```

Each method takes one string, including a reference. Bounds and lists or tuples
of alternative patterns are not supported yet.
