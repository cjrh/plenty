# Extend a list by transferring another list

`items.extend(other)` returns `Result[(), AllocError]`. It moves all elements
from another list of the same type after reserving space:

```plenty
def combined() -> Result[list[i64], AllocError]:
    mut items = list[i64].new()?
    more = [10, 20, 30]?
    items.extend(more)?
    Ok(items)

def main() -> Result[(), Failure]:
    print(combined()?)?
    Ok(())
```
```output
[10, 20, 30]
```

The source is consumed even when reservation fails; its elements are then cleaned
up and the destination stays unchanged. To retain a source, explicitly write
`items.extend(copy(source)?)`. References and general iterables are not
accepted as sources yet. The destination needs mutable access. Existing capacity
is reused, and moving elements does not clone or allocate their contents.
The literal in this example retains its usual terminal allocation policy; use
fallible constructors and insertions if source creation must also be recoverable.
