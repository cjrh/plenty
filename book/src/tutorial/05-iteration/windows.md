# Scan adjacent groups

Use `windows` when neighboring elements belong together. Each window borrows
its source, so reading it creates no list copy:

```plenty
def main() -> Result[(), AllocError | IoError]:
    readings = [10, 20, 30, 40]?
    for pair in readings.windows(2):
        print(pair)?
    Ok(())
```

```output
[10, 20]
[20, 30]
[30, 40]
```

Use `chunks` for groups that do not overlap. The final group may be shorter:

```plenty
def main() -> Result[(), AllocError | IoError]:
    readings = [10, 20, 30, 40, 50]?
    for group in readings.chunks(2):
        print(group)?
    Ok(())
```

```output
[10, 20]
[30, 40]
[50]
```

A width of zero or less yields nothing; windows wider than the source also
yield nothing. Text widths count Unicode scalars. For protocol bytes, use a
byte view instead. The views are read-only and keep their source borrowed;
change the source after their last use. These calls belong directly in a loop
source. See the [window contract](../../design/language/09-iteration/07-windows.md)
for lifetime and indexing rules.
