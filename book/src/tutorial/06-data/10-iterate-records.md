# Borrow records in a collection

For a list of records, `&items` yields shared references and `&mut items` yields
mutable references. The list cannot grow, shrink, or move during either loop:

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
