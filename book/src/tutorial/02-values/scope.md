# Keep branch-local names local

Assignments to an existing mutable binding survive a branch. New names declared
inside a branch belong to that branch:

```plenty
def main() -> Result[(), IoError | OverflowError[i64]]:
    mut price = 100
    discounted = True
    if discounted:
        discount = 20
        price = price - discount
    print(price)?
    Ok(())
```

```output
80
```

Declare a mutable binding before the branch when subsequent code needs the
updated value. For a new selected value, bind an inline conditional when each
branch is one expression. A block conditional also supports branch-local work:

```plenty
def main() -> Result[(), IoError]:
    discounted = True
    price = if discounted:
        discount = 20
        100 - discount
    else:
        100
    print(price)?
    Ok(())
```
```output
80
```

Only `price` survives the block; `discount` still belongs to its branch. This
keeps `price` immutable when it is selected once. The block must be the whole
right-hand side; see [conditional values](../../design/language/04-expressions/branches.md#an-if-as-a-statements-value).
