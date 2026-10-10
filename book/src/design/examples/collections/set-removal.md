# Remove set members

Use `values.discard(value)` to remove a member from a mutable set. It returns
`True` if the member was present and removed, or `False` if it was absent:

```plenty
def main() -> Result[(), Failure]:
    mut names = {"Ada", "Bea"}?
    name = "Ada"
    print(names.discard(name))?
    print(names.discard(name))?
    print(name)?
    print("Bea" in names)?
    print(len(names))?
    names.add(name)?
    print(len(names))?
    Ok(())
```

```output
True
False
Ada
True
1
2
```

The argument is observed, so `name` remains usable. Missing values are harmless,
and discarding a stored zero, `False`, or empty string still returns `True`.
Removal allocates nothing and preserves capacity for reuse. Like dictionary
removal, it has expected constant cost for well-distributed keys, apart from
hashing and cleanup; heavy hash collisions can still slow it down.
Sets still promise no iteration order.
Use an exclusive reference when removing members through a function parameter.

Collection equality compares contents; dictionary and set order do not matter.
