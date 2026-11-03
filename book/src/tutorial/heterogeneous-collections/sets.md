# Sets and different kinds of values

Sets have a narrower element contract than lists. A `set[T]` supports integers,
`bool`, `str`, `bytes`, and tuples recursively containing those types. Neither
an enum wrapper nor an anonymous sum is a supported set element, even when all
its payloads would be supported separately.

## Keep each kind in its own set

Suppose a selection contains numeric account IDs and textual group names.
Separate sets preserve both kinds without converting IDs into strings:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut account_ids: set[i64] = set()?
    mut group_names: set[str] = set()?
    account_ids.add(7)?
    account_ids.add(7)?
    group_names.add("editors")?
    group_names.add("7")?
    print(len(account_ids), len(group_names))?
    print(7 in account_ids, "7" in group_names)?
    Ok(())
```
```output
1 2
True True
```

The integer `7` and the name `"7"` remain distinct identities. Each set removes
its own duplicates and supports the ordinary
[set operations](../../design/facilities/01-builtins/sets.md).
Group the two sets in a class if they need to travel together.

## A sum does not make a set key

This list-style annotation is rejected:

```plenty-error
def main() -> Result[(), AllocError]:
    values: set[i64 | str] = set()?
    Ok(())
```
```error
set elements must be
```

If you need one ordered sequence of mixed values, use a
[list with a wrapper enum or anonymous sum](lists.md). A list preserves
duplicates; it does not acquire set semantics just because its element type is
a sum. Avoid converting unrelated kinds to one string type unless that text
representation is the identity your application intends.
