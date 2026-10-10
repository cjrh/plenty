# Borrowing elements in a loop

Use `&items` to inspect owned list elements without moving or copying them.
Each loop variable is a shared reference. Use `&mut items` to get mutable
references, including for scalar elements. The list itself cannot grow, shrink,
or be moved during either loop. Shared loops over copyable elements still yield
values, as they did before.

```plenty
class Score:
    value: i64

def main() -> Result[(), Failure]:
    mut scores = [Score(2), Score(5)]?
    for score in &mut scores:
        score.value = score.value + 1
    print([score.value for score in &scores]?)?
    mut numbers = [10, 20]?
    for number in &mut numbers:
        *number = *number + 2
    print(numbers)?
    Ok(())
```
```output
[3, 6]
[12, 22]
```
