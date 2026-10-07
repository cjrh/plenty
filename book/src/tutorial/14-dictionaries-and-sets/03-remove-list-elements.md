# Remove list elements

Lists also have `pop`. With no argument it removes the last element; an `i64`
index selects another position. Negative indices count from the end:

```plenty
def main() -> Result[(), Failure]:
    mut tasks = [[1]?, [2]?, [3]?]?
    match tasks.pop(1):
        case Some(task):
            mut work = task
            work.append(4)?
            print(work)?
        case Nothing:
            print("no task")?
    print(tasks)?
    print(tasks.pop())?
    print(tasks.pop(-1))?
    print(tasks.pop())?
    Ok(())
```

```output
[2, 4]
[[1], [3]]
Option[list[i64]].Some([3])
Option[list[i64]].Some([1])
Option[list[i64]].Nothing
```

As with dictionaries, `pop` transfers ownership and needs a mutable list or an
exclusive reference. Empty lists and out-of-range indices return `Nothing`.
Remaining elements stay in order, capacity is retained, and removal does not
allocate. Removing the last element is constant time; removing an earlier element
shifts the elements after it. The returned owner handles cleanup, so discarding
a successful result also drops its element.
