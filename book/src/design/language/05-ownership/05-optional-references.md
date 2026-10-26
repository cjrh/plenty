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

def main() -> Result[(), AllocError | IoError]:
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
- Returning the result from a function with the same origin, directly or from
  a local.
- Printing and `str.repr`, which show the referenced value:
  `Option[&str].Some("ada")`.

`unwrap_or(default)` is rejected: it would choose between two borrows. Use
`match`.

## Holding the result

A result can be bound to an immutable local. The local is a holder, not
storage: like a reference binding, it extends the origin loan to its last use.
Copying it shares that loan. A reference-carrying binding must hold a borrow,
so `o: Option[&str] = Nothing` is rejected.

A reference-carrying value is never stored. Tuples, lists and other
collections, class fields, enum payloads, generic type arguments, closure
captures, scoped workers, and values live across `yield` all reject it. Match
it first and store an owned copy or an index instead.

## Current limits

- `mut` locals and parameters of a reference-carrying type are rejected.
  Pass the reference after matching.
- A `match` used as a value cannot produce one.
- Patterns cannot match nested variants through the carried reference:
  `case Some(Some(v))` on `Option[&Option[i64]]` needs a second `match`.
- A returned reference from a temporary argument follows the
  [returned-reference rule](04-returned-references.md): the result cannot be
  bound to a local.
- Standard library lookups such as `dict.get` still return copies or trap;
  borrowed optional lookup methods are planned work.
