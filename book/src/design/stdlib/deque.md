# std.deque

`from std.deque import Deque` provides an owned generic ring buffer. It is
ordinary embedded Plenty code with private `list[Option[T]]` storage; it does
not change list indexing or `list.pop(0)` costs.

| Operation | Result | Contract |
|---|---|---|
| `Deque[T]()` | `Result[Deque[T], AllocError]` | Construct an empty deque |
| `queue.len()` | `i64` | Observe the live element count |
| `queue.is_empty()` | `bool` | Observe whether the count is zero |
| `queue.append(value)` | `Result[(), AllocError]` | Consume a value and append on the right |
| `queue.appendleft(value)` | `Result[(), AllocError]` | Consume a value and append on the left |
| `queue.pop()` | `Option[T]` | Remove and transfer the rightmost element |
| `queue.popleft()` | `Option[T]` | Remove and transfer the leftmost element |
| `queue.clear()` | `()` | Destroy elements from right to left, retaining capacity |

```plenty
from std.deque import Deque

def main() -> Result[(), AllocError | IoError]:
    mut queue = Deque[str]()?
    queue.append("b")?
    queue.append("c")?
    queue.appendleft("a")?
    print(queue.len(), queue.pop())?
    while True:
        match queue.popleft():
            case Some(item):
                print(item)?
            case Nothing:
                break
    Ok(())
```
```output
3 Option[str].Some("c")
a
b
```

Mutation requires a mutable owner or exclusive reference. Empty removals return
`Nothing`. Stored elements follow ordinary collection storage restrictions,
including affine owners; references, generators, and unit elements are unsupported.

End removal takes constant time apart from payload cleanup and allocates nothing.
Appending is amortized constant time; growth takes linear time and doubles
capacity, starting at eight slots. It reserves and initializes the full new
buffer before transferring any existing element. Allocation or capacity failure
preserves existing contents and drops the incoming value. Appending within
capacity, removing, and clearing allocate no runtime storage.

The deque moves and requires explicit `copy` for independent duplication. It
has no indexed access, iteration protocol, bounded capacity, rotation, or bulk
extension API. To consume it, repeatedly pop an end. See the runnable
[queue lesson](../../tutorial/04-collections/deque.md).

## Acknowledgements

- API modelled on Python's [`collections.deque`](https://docs.python.org/3/library/collections.html#collections.deque).
