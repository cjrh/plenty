# Update a set by transferring another set

Sets also support `update(source) -> Result[(), AllocError]`. Members already
present are kept, and missing members transfer from the source:

```plenty
def main() -> Result[(), Failure]:
    mut names = {"Ada", "Bea"}?
    more = {"Bea", "Cam"}?
    names.update(more)?
    print(len(names))?
    print("Ada" in names and "Bea" in names and "Cam" in names)?
    Ok(())
```
```output
3
True
```

The source must be a set of the same type and is consumed on both outcomes.
Use `names.update(copy(source)?)` to preserve it. Failed reservation leaves
the destination unchanged and cleans up the consumed source. Duplicate-only and
empty sources need no new runtime storage; existing capacity can cover new
members too. Set iteration order remains unspecified.
