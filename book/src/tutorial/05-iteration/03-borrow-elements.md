# Borrow elements while iterating

An exclusive loop lends each element in turn for mutation:

```plenty
def main() -> Result[(), Failure]:
    mut numbers = [10, 20]?
    for number in &mut numbers:
        *number = *number + 2
    print(numbers)?
    mut groups = [[1]?, [2, 3]?]?
    for group in &mut groups:
        group.append(4)?
    print(groups)?
    Ok(())
```
```output
[12, 22]
[[1, 4], [2, 3, 4]]
```

Use `&items` when inspection is enough. Copyable elements such as integers and
strings are yielded as values; owned elements such as inner lists are shared
references. Use `&mut items` for mutable references, including scalar elements.
The outer list cannot grow, shrink, or move during either loop. Afterward, the
owner is available again.
