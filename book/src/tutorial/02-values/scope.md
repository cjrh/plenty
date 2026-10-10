# Keep branch-local names local

Assignments to an existing mutable binding survive a branch. New names declared
inside a branch belong to that branch:

```plenty
def main() -> Result[(), IoError]:
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
updated value. Use a conditional expression if only the selected value needs
to survive the branch.
