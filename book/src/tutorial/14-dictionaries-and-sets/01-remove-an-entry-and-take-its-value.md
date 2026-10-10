# Remove an entry and take its value

`pop(key)` removes a dictionary entry and returns `Some(value)`, or `Nothing`
when the key is absent. It requires a mutable dictionary and transfers ownership
of the stored value, so it works with lists and classes too:

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

There is no hidden copy. The returned value remains valid if the dictionary is
dropped, and its new owner cleans it up normally. Discarding the result of `pop`
also cleans up the removed value, including any custom `__del__` method.

The operation preserves the order of remaining entries and reuses existing
storage without allocating. Reinserting a removed key puts it at the end.
Removal has expected constant cost for well-distributed keys, apart from hashing
and cleanup; heavy hash collisions can still slow it down. It accepts exactly one key;
there is no default argument. Sets use `discard`, described below.
