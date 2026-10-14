# Take the most urgent item first

Import `Heap` from `std.collections` for a priority queue. `pop` always removes
the least element, however the elements were pushed.

```plenty
from std.collections import Heap

def main() -> Result[(), Failure]:
    mut times = Heap[i64]()?
    for time in [40, 10, 30, 20]?:
        times.push(time)?
    print(*times.peek(), times.len())?
    while not times.is_empty():
        print(times.pop().unwrap())?
    print(times.pop())?
    Ok(())
```

```output
10 4
10
20
30
40
Option[i64].Nothing
```

Construction and `push` return `Result` with `AllocError`. `pop` returns
`Option` and transfers the element. `peek` borrows the first element and traps
on an empty heap, so test `is_empty()` first.
The trap report names the library operation and adds `note: called from` with
the position of your `peek()` call.

`Heap` orders integers, floats, and strings with `<`. For any other element,
`HeapBy` takes a function that says which of two elements leaves first:

```plenty
from std.collections import HeapBy

class Job:
    priority: i64
    name: str

def urgent(a: &Job, b: &Job) -> bool:
    a.priority > b.priority

def main() -> Result[(), Failure]:
    mut jobs = HeapBy(urgent)?
    jobs.push(Job(1, "sweep"))?
    jobs.push(Job(9, "deploy"))?
    jobs.push(Job(5, "review"))?
    while not jobs.is_empty():
        job = jobs.pop().unwrap()
        print(job.priority, job.name)?
    Ok(())
```

```output
9 deploy
5 review
1 sweep
```

The heap owns its jobs; `pop` moves one out without copying it. Elements that
compare equal leave in no promised order. See
[the module reference](../../design/stdlib/collections.md) for costs and limits.
