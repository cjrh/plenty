# Remove an entry and take its value

`pop(key)` removes a dictionary entry and returns `Some(value)`, or `Nothing`
when the key is absent. It requires a mutable dictionary and transfers ownership
of the stored value, including nested collections:

```plenty
def main() -> Result[(), Failure]:
    mut groups = {"ready": [1, 2]?, "waiting": [3]?}?
    match groups.pop("ready"):
        case Some(items):
            mut work = items
            work.append(4)?
            print(work)?
        case Nothing:
            print("no work")?
    print(groups)?
    print(groups.pop("missing"))?
    Ok(())
```

```output
[1, 2, 4]
{"waiting": [3]}
Option[list[i64]].Nothing
```

The returned owner remains valid if the dictionary is dropped. Discarding a
successful `pop` result also cleans up its removed value.

Removal preserves the order of remaining entries without allocating. Reinserting
a removed key puts it at the end. `pop` takes one key and has no default argument.

## Remove list elements

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

List `pop` needs mutable access. Empty lists and out-of-range indices return
`Nothing`. Removal retains capacity, preserves element order, and does not
allocate. Removing the last element is constant time; removing an earlier one
shifts subsequent elements.
