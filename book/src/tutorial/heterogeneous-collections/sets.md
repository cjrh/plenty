# Sets and different kinds of values

Sets accept named enums when every payload is a supported key type: integers,
`bool`, `str`, `bytes`, or tuples, classes and named enums of those types, without custom cleanup. The variant
is part of the identity, so different variants with equal payloads remain distinct.

```plenty
enum Selection:
    Account(i64)
    Group(str)
    All

def main() -> Result[(), AllocError | IoError]:
    mut selected: set[Selection] = set()?
    selected.add(Selection.Account(7))?
    selected.add(Selection.Group("7"))?
    selected.add(Selection.Account(7))?
    print(len(selected))?
    print(Selection.Account(7) in selected)?
    Ok(())
```
```output
2
True
```

This is one element type with several alternatives. Enums carrying collections,
floats, boxes or classes with custom cleanup cannot be set elements. Anonymous sums remain
unsupported, even when each member is supported separately.

## Class payloads

A wrapper can also carry classes whose fields are supported keys. Ordinary
classes move into the set; use a borrowed probe to keep a lookup value:

```plenty
class Account:
    region: str
    number: i64

enum Selection:
    Account(Account)
    Group(str)

def main() -> Result[(), AllocError | IoError]:
    selected = {Selection.Account(Account("eu", 7)), Selection.Group("editors") }?
    probe = Selection.Account(Account("eu", 7))
    print(&probe in selected)?
    for item in &selected:
        match item:
            case Selection.Account(account):
                print(account.region, account.number)?
            case Selection.Group(group):
                print(group)?
    Ok(())
```
```output
True
eu 7
editors
```

The lookup compares fields rather than object addresses. Borrowed iteration
keeps the stored keys immutable. Set algebra creates independent key copies;
its result can outlive either input. See [sets](../../design/facilities/01-builtins/sets.md).

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

## Small enums without payloads

Use ordinary sets for flags or selections too. An enum with no payloads and at
most 64 variants automatically gets compact bitset storage:

```plenty
enum Permission:
    Read
    Write
    Execute

def main() -> Result[(), AllocError | IoError]:
    mut granted = {Permission.Read}?
    granted.add(Permission.Write)?
    required = {Permission.Read, Permission.Write}?
    print(required.issubset(&granted))?
    granted.discard(Permission.Write)
    print(Permission.Write in granted)?
    Ok(())
```
```output
True
False
```

Keep the usual `?` handling: constructing a set still allocates its owner.
Adding members to this representation needs no further allocation. The same
programming style works for larger enums and payload-carrying enums, which use
hash sets. See [small enum bitsets](../../design/facilities/01-builtins/sets.md#small-enum-bitsets).
