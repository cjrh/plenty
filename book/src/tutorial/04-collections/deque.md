# Keep a queue with two ends

Use `std.deque.Deque` for queues and breadth-first traversal. Removing the
first element does not shift the remaining elements.

```plenty
from std.deque import Deque

def main() -> Result[(), Failure]:
    mut queue = Deque[(i64, i64)]()?
    queue.append((0, 0))?
    queue.append((0, 1))?
    queue.appendleft((-1, 0))?
    print(queue.len())?
    while not queue.is_empty():
        print(queue.popleft().unwrap())?
    print(queue.pop())?
    Ok(())
```
```output
3
(-1, 0)
(0, 0)
(0, 1)
Option[tuple[i64, i64]].Nothing
```

Construction and appending return Result with `AllocError`. End removals
return Option and transfer ownership. Appending takes ownership even on
failure, while the existing queue remains intact. Growth is occasional;
appending within capacity and removing an end allocate nothing. `clear()`
drops all remaining values and retains the buffer for reuse.

See [the module reference](../../design/stdlib/deque.md) for exact operations
and costs. A deque is owned; it has no borrowed iterator or indexed API yet.
