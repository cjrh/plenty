# Collection literals

Collection literals directly return `Result[collection, AllocError]`:

```plenty
def build() -> Result[list[list[i64]], AllocError]:
    [[1, 2]?, [3]?]

def main() -> Result[(), Failure]:
    print({"answer": 42}?)?
    print(build()?)?
    Ok(())
```
```output
{"answer": 42}
[[1, 2], [3]]
```

Construction stops at its first allocation failure, drops the partial collection,
and skips later entries. The nested `?` operations extract each inner list and
propagate its failure to `build`. There is no prefix `try` keyword.
Empty displays need context: `values: Result[list[i64], AllocError] = []`, or
`values: list[i64] = []?` inside a function returning `Result[..., AllocError]`.
