# Borrow elements while iterating

An exclusive loop lends each element in turn. Updating that reference changes
the existing element without building a second list:

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
The outer list cannot grow, shrink, or move while either loop borrows it. The
borrow ends after the loop, so the owner can be printed or changed afterward.

The same rules apply to class instances, which the next part introduces.
