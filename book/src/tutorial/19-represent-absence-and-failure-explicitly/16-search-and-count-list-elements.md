# Search and count list elements

For lists of integers, floats, booleans, or strings, these queries borrow their
inputs and allocate nothing:

```plenty
def main() -> Result[(), Failure]:
    names = ["Ada", "Lin", "Ada"]?
    print(names.count("Ada"))?
    print(names.find("Ada"))?
    print(names.rfind("Ada"))?
    print(names.find("missing"))?
    Ok(())
```

```output
2
Option[i64].Some(0)
Option[i64].Some(2)
Option[i64].Nothing
```

Searches return optional zero-based positions. The query must match the element
type exactly. Float comparisons follow IEEE rules: NaN never matches, and positive
and negative zero match each other. Searching lists of aggregates is not yet supported.
