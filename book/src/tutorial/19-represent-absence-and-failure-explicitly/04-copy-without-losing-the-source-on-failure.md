# Copy without losing the source on failure

`copy(value)` returns
`Result[T, AllocError]`. It observes the source, so your original value remains
usable whether copying succeeds or fails. Use `?` to keep the success path short:

```plenty
def extended(source: &list[i64]) -> Result[list[i64], AllocError]:
    mut result = copy(source)?
    result.append(30)?
    Ok(result)

def main() -> Result[(), Failure]:
    original = [10, 20]?
    print(extended(&original)?)?
    print(original)?
    Ok(())
```

```output
[10, 20, 30]
[10, 20]
```

This also works for nested collections, ordinary classes, and enum payloads.
If copying a later field or element fails, the partial copy is cleaned up and
the original remains unchanged. Scalars and immutable values, including strings,
need no allocation to copy; their existing immutable storage can be shared.

Generators and classes with
custom cleanup cannot be copied, including when nested in another value.
The operation covers duplication itself. In `copy([1, 2]?)?`, the first `?`
handles construction and the second handles copying.
