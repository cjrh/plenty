# Take the most urgent item first

Import `Heap` from `std.collections` for a priority queue. `pop` always removes
the least element, however the elements were pushed.

```plenty
from std.collections import Heap

def main() -> Result[(), AllocError | IoError]:
    mut times = Heap[i64]()?
    for time in [40, 10, 30, 20]?:
        times.push(time)?
    print(times.peek(), times.len())?
    while not times.is_empty():
        match times.pop():
            case Some(value):
                print(value)?
            case Nothing:
                break
    print(times.pop())?
    Ok(())
```

```output
Option[i64].Some(10) 4
10
20
30
40
Option[i64].Nothing
```

Construction and `push` return `Result` with `AllocError`. `pop` returns
`Option` and transfers the element. `peek` returns a copy of the first element
without removing it, or `Nothing` when the heap is empty.

`Heap` orders integers, floats, and strings with `<`. For any other element,
`HeapBy` takes a function that says which of two elements leaves first:

```plenty
from std.collections import HeapBy

class Job:
    priority: i64
    name: str

def urgent(a: &Job, b: &Job) -> bool:
    a.priority > b.priority

def main() -> Result[(), AllocError | IoError]:
    mut jobs = HeapBy(urgent)?
    jobs.push(Job(1, "sweep"))?
    jobs.push(Job(9, "deploy"))?
    jobs.push(Job(5, "review"))?
    match jobs.peek_ref():
        case Some(job):
            print("next:", job.name)?
        case Nothing:
            pass
    while not jobs.is_empty():
        match jobs.pop():
            case Some(job):
                print(job.priority, job.name)?
            case Nothing:
                break
    Ok(())
```

```output
next: deploy
9 deploy
5 review
1 sweep
```

The heap owns its jobs; `pop` moves one out without copying it. A `Job` cannot
be copied, so `peek` is rejected for this heap; `peek_ref` borrows the first job
instead, as in [returning a borrow that may be absent](../06-data/borrowing-methods.md#return-a-borrow-that-may-be-absent).
The heap cannot change while that borrow is in use. Elements that
compare equal leave in no promised order. See
[the module reference](../../design/stdlib/collections.md) for costs and limits.
