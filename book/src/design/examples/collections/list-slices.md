# Take a list slice

A [slice](../../language/04-expressions/slices.md) `items[start:stop:step]`
returns a new list inside a `Result`. Start is included and stop is excluded.
Negative bounds count from the end, omitted bounds run to the ends, a negative
step walks backwards, and bounds outside the list clamp to its ends:

```plenty
def middle(items: &list[i64]) -> Result[list[i64], AllocError]:
    items[1:-1]

def main() -> Result[(), AllocError | IoError]:
    items = [10, 20, 30, 40]?
    print(middle(&items)?)?
    print(items[-100:100]?)?
    print(items[3:1]?)?
    print(items[::-2]?)?
    print(items)?
    Ok(())
```
```output
[20, 30]
[10, 20, 30, 40]
[]
[40, 20]
[10, 20, 30, 40]
```

Bounds must be `i64`, and the step is a `NonZero[i64]`. Even an empty result
can fail to allocate its list header; use `?` or `match` to handle `AllocError`.

The slice copies each selected element with `copy()`, so nested lists are
duplicated and the source keeps its elements. Element types that cannot be
copied, such as `std.bytes.Bytes`, are rejected. For those, call
`items.slice(start, stop)` on an owned temporary, such as a list returned by a
function: it transfers the selected elements and drops the rest. Use
`copy(items)?.slice(start, stop)` only when the elements can be copied anyway.
If allocation fails, the temporary is cleaned up in full; a borrowed source
remains unchanged.
