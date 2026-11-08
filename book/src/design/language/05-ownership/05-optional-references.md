# Optional and fallible references

A function may return `Option[&T]`, `Option[&mut T]`, `Result[&T, E]`, or
`Result[&mut T, E]`: a borrow that may be absent or may fail. The success
payload may also be another such enum, as in `Result[Option[&T], E]`. These
*reference-carrying* results follow the
[returned-reference rule](04-returned-references.md): the payload borrows the
first reference parameter, a function without one is rejected, and
`Option[&mut T]` requires an `&mut` origin.

```plenty
class Item:
    name: str
    count: i64

def find(items: &mut list[Item], name: &str) -> Option[&mut Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut items = [Item("ada", 1)]?
    match find(&mut items, "bob"):
        case Some(item):
            item.count += 1
        case Nothing:
            items.append(Item("bob", 1))?
    match find(&mut items, "ada"):
        case Some(item):
            item.count += 1
        case Nothing:
            pass
    print(items[0].count)?
    print(len(items))?
    Ok(())
```

```output
2
2
```

`Nothing`, `Err(e)`, and `Some` or `Ok` around such a value hold no loan and
are always accepted. Every other return path must borrow the origin, as for
`&T` results. `Err` payloads cannot hold references: copy the part of the input
that the error reports.

## Using the result

The caller's origin loan lasts until the last use of the result or of any
reference taken from it. A `Nothing` or `Err` arm holds no payload, so it may
modify the origin. These forms take the reference out or inspect the result:

- `match result` binds the payload reference directly; `match &result` is
  rejected because the result already holds a reference.
- `?` propagates `Nothing` or `Err` and produces the reference.
- `unwrap()`, `ok_or(e)`, `is_some()`, `is_none()`, `is_ok()`, and `is_err()`.
- `unwrap_or(&default)`, which produces the payload reference or the default
  reference. The result keeps both owners borrowed while it is used, as an
  [`if` or `match` value](#choosing-with-if-or-match) keeps every arm's
  borrow.
- Returning the result from a function with the same origin, directly or from
  a local.
- Printing and `str.repr`, which show the referenced value:
  `Option[&str].Some("ada")`.

A pattern below the payload matches the referent, as a match on a reference
does: `case Some(Item(name=n, count=3))` on `Option[&Item]` tests the item's
field, and `case Some(Some(v))` on `Option[&mut Option[i64]]` binds `v` as a
`&mut i64` into the inner option. Each name bound below the payload is a
reference that keeps the origin borrowed, like the payload itself. Names bound
below one exclusive payload, as in `case Some(Item(name=n, count=k))` on
`Option[&mut Item]`, are disjoint, so each can be written in the arm.

```plenty
class Item:
    name: str
    count: i64

def find(items: &list[Item], name: &str) -> Option[&Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def first(values: &mut list[Option[i64]]) -> Option[&mut Option[i64]]:
    for value in values:
        return Some(value)
    Nothing

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    items = [Item("ada", 3)]?
    other = Item("guest", 0)
    print(find(&items, "bob").unwrap_or(&other).name)?
    match find(&items, "ada"):
        case Some(Item(name=name, count=3)):
            print(name, "has three")?
        case Some(_) | Nothing:
            pass
    mut values = [Some(1)]?
    match first(&mut values):
        case Some(Some(value)):
            *value += 10
        case Some(Nothing) | Nothing:
            pass
    print(values)?
    Ok(())
```

```output
guest
ada has three
[Option[i64].Some(11)]
```

With a borrowed payload, `map_or`, `map_or_else` and `unwrap_or_else` mean
the `match` they abbreviate. The mapper takes the payload reference, as in
`def (item: &Item) -> i64`, and `map_or` evaluates its default first. A
result that is itself a reference, such as `map_or(&other.name, name_of)`
with `def name_of(item: &Item) -> &str`, keeps both owners borrowed while it
is used, like `unwrap_or`. Arguments evaluate left to right before the
selected callback runs. A fallback of `map_or_else` or `unwrap_or_else` takes
no reference parameter, since a `Result` error cannot hold one, so it cannot
return a borrow; use `map_or` or `unwrap_or(&default)` for a borrowed default.

```plenty
class Item:
    name: str
    count: i64

def find(items: &list[Item], name: &str) -> Option[&Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def name_of(item: &Item) -> &str:
    &item.name

def main() -> Result[(), AllocError | IoError]:
    items = [Item("ada", 3)]?
    other = Item("guest", 0)
    print(find(&items, "ada").map_or(0, def (item: &Item) -> i64: item.count))?
    print(find(&items, "bob").map_or(&other.name, name_of))?
    Ok(())
```

```output
3
guest
```

## Holding the result

A result can be bound to an immutable local. The local is a holder, not
storage: like a reference binding, it extends the origin loan to its last use.
Copying it shares that loan.

A holder initialized with a value that holds no reference, such as
`Nothing`, `Err(e)`, or `Some(Nothing)`, borrows nothing. Give it a type
annotation, since `Nothing` alone does not name the payload type. Its origin
stays free to change while the holder is live, and any function with a
reference-carrying result may return it. Matching, predicates, `?`, `unwrap()`, printing and
copying work as for any holder; a payload reference taken from it can never
exist at run time. Each such holder uses one extra hidden local slot, which
counts toward the limit of 256 parameter and local slots per function.

```plenty
def first_long(words: &mut list[str], limit: i64) -> Option[&str]:
    missing: Option[&str] = Nothing
    words[0] = "changed"
    for i in range(len(words)):
        if words[i].byte_len() > limit:
            return Some(&words[i])
    missing

def main() -> Result[(), AllocError | IoError]:
    mut words = ["a", "bc"]?
    print(first_long(&mut words, 3))?
    print(first_long(&mut words, 10))?
    failed: Result[&str, i64] = Err(4)
    print(failed.is_err())?
    Ok(())
```

```output
Option[&str].Some("changed")
Option[&str].Nothing
True
```

A `mut` holder declared with a borrowing value can be reassigned, following
[reference rebinding](03-reference-rebinding.md): later values must borrow
within the place its first value borrowed. A `mut` holder that starts empty
borrows nothing until it is assigned a borrowing value, and its values may
borrow different places and owners. Every use of the holder keeps every value
assigned to it borrowed, on each path where one of those assignments ran,
including through a loop to a use written before the assignment. A copy of
the holder keeps them borrowed too, and so does another started-empty holder
assigned its value: a use of that holder keeps every value assigned to the
first one borrowed, before or after the copy.

```plenty
class Item:
    name: str

def longest(items: &list[Item]) -> Option[&Item]:
    mut best: Option[&Item] = Nothing
    mut size = 0
    for item in items:
        if item.name.byte_len() > size:
            size = item.name.byte_len()
            best = Some(item)
    best

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada"), Item("grace")]?
    mut last: Option[&Item] = Nothing
    items.append(Item("bob"))?
    for item in &items:
        last = Some(item)
    print(last)?
    print(longest(&items))?
    Ok(())
```

```output
Option[&Item].Some(Item(name="bob"))
Option[&Item].Some(Item(name="grace"))
```

`items` can change before `last` is first assigned, but not while `last` is
live afterwards. A `return` of a `mut` holder that is still empty can run
again after a later assignment in a loop, so once a function returns such a
holder, every assignment to it must borrow the origin parameter, as must
every assignment to a holder whose value it was assigned.

Because earlier values stay borrowed, an exclusive holder cannot take a second
exclusive borrow of the same owner while it is used afterwards, even in
straight-line code. Use a separate binding per value:

```plenty-error
class Item:
    name: str
    count: i64

def find_mut(items: &mut list[Item], name: &str) -> Option[&mut Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 1), Item("bob", 2)]?
    mut cur: Option[&mut Item] = Nothing
    cur = find_mut(&mut items, "ada")
    print(cur)?
    cur = find_mut(&mut items, "bob")
    print(cur)?
    Ok(())
```

```error
`cur` started empty, so it keeps every value assigned to it borrowed; use a separate binding per value
```

Assigning `Nothing` or `Err(e)` does not release the earlier values. To
choose between owners, start the holder empty:

```plenty
class Item:
    name: str

def main() -> Result[(), AllocError | IoError]:
    staff = [Item("ada")]?
    guests = [Item("bob")]?
    mut pick: Option[&Item] = Nothing
    if len(staff) > 1:
        pick = Some(&staff[0])
    else:
        pick = Some(&guests[0])
    print(pick)?
    Ok(())
```

```output
Option[&Item].Some(Item(name="bob"))
```

A holder declared with a borrowing value is rejected when assigned a borrow of
another owner, with "reference binding `pick` can be reassigned only to a
reference into the owner it first borrowed; to choose between owners, start it
empty".

## Choosing with `if` or `match`

An `if` or `match` used as a value may produce a reference-carrying value.
Each arm may borrow a different owner. The checker does not know which arm
ran, so while the result is used, every arm's owner stays borrowed:

```plenty
class Item:
    name: str
    count: i64

def find(items: &list[Item], name: &str) -> Option[&Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def main() -> Result[(), AllocError | IoError]:
    mut staff = [Item("ada", 1)]?
    guests = [Item("bob", 2)]?
    selected = if len(guests) > 0:
        find(&guests, "bob")
    else:
        find(&staff, "ada")
    print(selected)?
    staff.clear()
    Ok(())
```

```output
Option[&Item].Some(Item(name="bob", count=2))
```

Modifying `staff` before the last use of `selected` is rejected, even though
the `if` chose `guests`:

```plenty-error
class Item:
    name: str

def find(items: &list[Item], name: &str) -> Option[&Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def main() -> Result[(), AllocError | IoError]:
    mut staff = [Item("ada")]?
    guests = [Item("bob")]?
    selected = if len(guests) > 0:
        find(&guests, "bob")
    else:
        find(&staff, "ada")
    staff.clear()
    print(selected)?
    Ok(())
```

```error
`selected` holds this arm's value: an `if` or `match` value keeps the value of every arm borrowed
```

An arm cannot produce a borrow of an owner it declares, or of a temporary,
since that owner is dropped when the arm ends. A returned `if` or `match`
value follows the [returned-reference rule](04-returned-references.md) in each
arm. A `checked:` block cannot produce a reference-carrying value.

An arm's value cannot yet be a plain reference such as `&items[0]`, although
`unwrap_or(&default)` and `map_or` produce one from two owners: a binding
cannot take its reference from an `if` or `match` value
(issue #897, [backlog](../../../backlog.md) B11). Use `unwrap_or` or `map_or`, or keep
the reference inside an `Option[&T]`.

Each evaluation borrows afresh. In a loop, a result used only within its
iteration does not keep the owners borrowed into the next one. A copy that
outlives the iteration, such as one assigned to a `mut` holder declared
before the loop, keeps every arm's owner borrowed wherever the copy is live.
The loop may still evaluate the `if` again into the same binding.

## Passing a holder

A parameter may have a reference-carrying type, such as
`found: Option[&Item]`. It borrows what the argument borrows, and counts as a
reference parameter when choosing the origin of a returned borrow:
`def pick(found: Option[&Item], other: &Item) -> &Item` may return only a
borrow from `found`'s payload. An `Option[&mut T]` origin allows an
`Option[&mut T]` result.

```plenty
class Item:
    name: str
    count: i64

def find(items: &mut list[Item], name: &str) -> Option[&mut Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def reset(found: Option[&mut Item]) -> ():
    match found:
        case Some(item):
            item.count = 0
        case Nothing:
            pass

def name_of(found: Option[&Item]) -> Option[&str]:
    match found:
        case Some(item):
            Some(&item.name)
        case Nothing:
            Nothing

def main() -> Result[(), AllocError | IoError]:
    mut items = [Item("ada", 3)]?
    held = find(&mut items, "ada")
    reset(held)
    reset(held)
    reset(Nothing)
    shared: Option[&Item] = Some(&items[0])
    print(name_of(shared))?
    print(items[0].count)?
    Ok(())
```

```output
Option[&str].Some("ada")
0
```

Passing a holder reborrows its loan for the call, as passing a reference
does. A shared holder stays usable. An exclusive holder can be passed again
only after the call and any result that keeps its borrow are no longer used,
so two live `&mut` capabilities never exist. Passing `Nothing` or `Err(e)`
lends nothing. Generic helpers (`def kept[T](found: Option[&T]) -> Option[&T]`),
methods, and `Callable[[Option[&T]], R]` signatures take these parameters too.
A generic helper that takes the callable by reference, as in
`apply[F: Callable[[Option[&T]], Option[&T]]](f: &F, found: Option[&T])`,
cannot return `found`'s borrow: `f` is its first reference parameter, so
under the [returned-reference rule](04-returned-references.md) a returned
borrow must come from `f`. Put `found` first.
A parameter cannot borrow a holder, as in `&Option[&T]`; pass the holder
itself. Generators cannot take one.

A reference-carrying value is never stored. Tuples, lists and other
collections, class fields, enum payloads, generic type arguments, closure
captures, scoped workers, and values live across `yield` all reject it. Match
it first and store an owned copy or an index instead.

## Current limits

- A `mut` holder declared with a borrowing value stays within the place that
  value borrowed; start it empty to choose between owners.
- A `mut` holder that started empty keeps every value ever assigned to it
  borrowed while it is used, even after a later assignment replaced that
  value.
- Copying one name bound below an exclusive payload out of its arm, as in
  `kk = k` or `keep = Some(k)` after `case Some(Item(name=n, count=k))`,
  keeps the other names bound by that pattern unusable while the copy is
  live. The names share one loan; use the copy last, or write through `k`
  directly.
- A parameter cannot borrow a reference-carrying value, as in
  `&Option[&Item]`, since references to references are not supported; pass
  the `Option[&Item]` itself, which lends its borrow to the call.
- A generator cannot take a reference-carrying parameter, as it cannot take
  a reference parameter: its frame would store the borrow, which needs
  stored references (backlog B11).
- A returned reference from a temporary argument follows the
  [returned-reference rule](04-returned-references.md): the result cannot be
  bound to a local.
- Standard library lookups such as `dict.get` still return copies or trap;
  borrowed optional lookup methods are planned work.
