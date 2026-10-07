# Combine sets without consuming them

`union` returns an independent set inside a `Result`. Both inputs are borrowed:

```plenty
def merged(a: &set[i64], b: &set[i64]) -> Result[set[i64], AllocError]:
    a.union(b)

def main() -> Result[(), Failure]:
    a = {1, 2}?
    b = {2, 3}?
    match merged(&a, &b):
        case Ok(values):
            print(len(values))?
            print(1 in values and 3 in values)?
        case Err(error):
            print(error)?
    print(len(a))?
    Ok(())
```

```output
3
True
2
```

An allocation failure preserves the inputs. Set iteration order is unspecified.

Find common members with `intersection`:

```plenty
def main() -> Result[(), Failure]:
    match {1, 2}?.intersection({2, 3}?):
        case Ok(common):
            print(len(common))?
            print(2 in common)?
        case Err(error):
            print(error)?
    Ok(())
```

```output
1
True
```

Use `difference` to remove another set's members from a new result:

```plenty
def main() -> Result[(), Failure]:
    wanted = {1, 2, 3}?
    completed = {2, 3, 4}?
    match wanted.difference(completed):
        case Ok(remaining):
            print(len(remaining))?
            print(1 in remaining)?
        case Err(error):
            print(error)?
    print(len(wanted))?
    Ok(())
```

```output
1
True
3
```

The receiver determines which members can appear in the result. Both inputs remain usable.

`symmetric_difference` keeps members found in exactly one of the two inputs:

```plenty
def main() -> Result[(), Failure]:
    match {1, 2}?.symmetric_difference({2, 3}?):
        case Ok(changed):
            print(len(changed))?
            print(1 in changed and 3 in changed)?
            print(2 in changed)?
        case Err(error):
            print(error)?
    Ok(())
```

```output
2
True
False
```
