# Keep distinct values in a set

A `set[T]` stores each distinct value once. Its elements can be integers,
booleans, or strings:

```plenty
def main() -> Result[(), Failure]:
    mut names: set[str] = set()?
    names.add("Ada")?
    names.add("Ada")?
    print(len(names))?
    print("Ada" in names)?
    names.add("Grace")?
    print(names.discard("Ada"))?
    print(names.discard("Ada"))?
    print(len(names))?
    Ok(())
```

```output
1
True
True
False
1
```

`add` needs mutable access and returns an allocation result.
`discard` removes a member without allocating and returns whether it was
present.

Use `set[str]()?` for a typed empty set, or provide an annotation as above.
A display such as `{"Ada", "Grace"}?` builds a set directly. `{}` is an
empty dictionary. Sets have no promised iteration order.

Dictionary and set equality compare contents independently of order.
