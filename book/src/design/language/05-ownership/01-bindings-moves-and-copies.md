# Bindings, moves, and copies

Bindings and mutable reassignment follow the
[declaration and assignment rules](../04-expressions/assignment.md). Ordinary
value use either copies or transfers ownership according to the value's type.

Scalars, immutable strings, ranges, named function values, and aggregates of
copyable components copy on ordinary value use. Strings may share immutable
storage. Class instances are affine, including a record of integers, unless
the class is declared [`@copy`](../03-types/classes.md#copy-classes).
Collections, boxes, generators, captured closures, and aggregates containing
affine components also move on assignment, owned argument passing, and return.
Inserting an affine value into a collection or capturing it transfers ownership.
A moved-from binding cannot be read until a permitted reassignment initializes it.

`copy(value)` observes its argument and returns `Result[T, AllocError]` containing
an independent value. It recursively duplicates owned mutable contents;
immutable strings can share storage. A nested
[`Shared`](../../facilities/01-builtins/shared.md) handle is not duplicated:
the copy holds another owner of the same content, as `share()` would.
`copy(handle)` on a handle itself is rejected in favour of `handle.share()`.
Copying does not supply mutable aliases.
Generators, captured closures, recursive data, and types containing custom
destruction cannot be copied. [Allocation](../08-errors/02-allocation-and-traps.md)
and [destruction](07-destruction.md) describe failure and cleanup.

Only paths that continue contribute to availability at a branch join. Moving on
some continuing paths makes a binding possibly moved; moving on all makes it
moved. Reinitializing a mutable binding restores availability. Every loop
backedge, including `continue`, must preserve outer owners available at loop
entry. Moving and then reinitializing is accepted; carrying the move to a
backedge is rejected. Break paths join the zero-iteration path.

## Moving fields

An affine field can move out of a class instance held by an owned binding,
including an owned `self` receiver. The path from the binding to the field may
pass through further class fields, as in `outer.inner.resource`. Availability
is then tracked per field, with the same rules as bindings:

- The moved field, and any place inside it, cannot be used until it is assigned.
- The other fields remain usable and can move out too.
- The binding cannot be used as a whole while a field is moved: it cannot be
  moved, passed, returned, borrowed, or used as a method receiver. Assigning
  the field (`value.field = replacement`) or the whole binding restores it.
- At a branch join, a field moved on some paths is possibly moved. A field
  moved in a loop must be assigned again before the backedge.
- When the binding goes out of scope, or a path returns early, only the fields
  still initialized are destroyed, each exactly once.

```plenty
class Resource:
    name: str

    def __del__(self) -> ():
        print("close", self.name).unwrap()

class Pair:
    left: Resource
    right: Resource

def consume(resource: Resource) -> ():
    print("consume", resource.name).unwrap()

def main() -> ():
    mut pair = Pair(Resource("a"), Resource("b"))
    consume(pair.left)
    print(pair.right.name).unwrap()
    pair.left = Resource("c")
    whole = pair
    print("end").unwrap()
```
```output
consume a
close a
b
end
close c
close b
```

Using the binding as a whole before the field is assigned is rejected:

```plenty-error
class Resource:
    name: str

class Pair:
    left: Resource
    right: Resource

def main() -> ():
    pair = Pair(Resource("a"), Resource("b"))
    left = pair.left
    whole = pair
```
```error
use of partly moved binding `pair`: `pair.left` is moved; assign `pair.left` before using `pair` as a whole
```

Moving out needs no `mut` binding, as moving the whole value does not, but
assigning the field back does. Fields cannot move out:

- through a reference, including bare `self` and `&mut` receivers; use
  [`replace`](../04-expressions/assignment.md#replacing-a-field), which
  transfers the old value while leaving the field initialized;
- of a class that defines `__del__`, which receives the whole object and so
  needs every field initialized; `replace` works there too;
- through a `Box`, a `Shared` handle, a collection element, a closure capture,
  or a temporary.

Tuple elements move by unpacking the whole tuple, and enum payloads by
consuming `match`. Use an explicit copy or a collection removal operation for
collection elements. Two list elements can be exchanged in place with
[`swap`](../../facilities/01-builtins/lists.md).
