# Take a list slice

`items.slice(start, stop)` returns a new list inside a `Result`. Start is
included and stop is excluded. Negative bounds count from the end, and bounds
outside the list clamp to its ends:

```plenty
def middle(items: &list[i64]) -> Result[list[i64], AllocError]:
    items.slice(1, -1)

def main() -> Result[(), Failure]:
    items = [10, 20, 30, 40]?
    print(middle(&items))?
    print(items.slice(-100, 100))?
    print(items.slice(3, 1))?
    print(items)?
    Ok(())
```

```output
Result[list[i64], AllocError].Ok([20, 30])
Result[list[i64], AllocError].Ok([10, 20, 30, 40])
Result[list[i64], AllocError].Ok([])
[10, 20, 30, 40]
```

Both bounds must be `i64`. Slice syntax such as `items[1:3]`, omitted bounds,
and steps are not implemented yet. Even an empty result can fail to allocate
its list header; use `?` or `match` to handle `AllocError`.

Strings and immutable enums in the slice share their immutable storage and
outlive the original list. For owned elements such as nested lists or classes,
the receiver must be an owned temporary. Use
`copy(items)?.slice(start, stop)` to preserve the original, or call the
method on a list returned by a function to transfer selected elements. Unselected
elements of that temporary are dropped after the call. If allocation fails,
the temporary is cleaned up in full; a borrowed source remains unchanged.
