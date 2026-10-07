# 2. Name values

Use `=` to create a binding. Plenty infers the type of a local value, so you
usually do not need an annotation:

```plenty
def main() -> Result[(), IoError]:
    # Comments begin with a hash.
    price = 20
    quantity = 3
    total = price * quantity
    print(total)?
    Ok(())
```

```output
60
```

Bindings are immutable by default. Assignment is not a way to silently change
an existing value or its type:

```plenty-error
def main() -> ():
    score = 10
    score = 11
```

The diagnostic includes:

```error
`score` is immutable; declare it with `mut`
```

Use `mut` when a value needs to change. Write it once, at the declaration:

```plenty
def main() -> Result[(), IoError]:
    mut score: i64 = 10
    score = score + 5
    print(score)?
    Ok(())
```

```output
15
```

A mutable binding still keeps its original type. It cannot start as an integer
and later become a string. `score: i64` explicitly names the type; leaving out
the annotation would infer the same type here.
