# Bindings, moves, and copies

Bindings and mutable reassignment follow the
[declaration and assignment rules](../04-expressions/assignment.md). Ordinary
value use either copies or transfers ownership according to the value's type.

Scalars, immutable strings, ranges, named function values, and aggregates of
copyable components copy on ordinary value use. Strings may share immutable
storage. Every class instance is affine, including a record of integers.
Collections, boxes, generators, captured closures, and aggregates containing
affine components also move on assignment, owned argument passing, and return.
Inserting an affine value into a collection or capturing it transfers ownership.
A moved-from binding cannot be read until a permitted reassignment initializes it.

`copy(value)` observes its argument and returns `Result[T, AllocError]` containing
an independent value. It recursively duplicates owned mutable contents;
immutable strings can share storage. Copying does not supply mutable aliases.
Generators, captured closures, recursive data, and types containing custom
destruction cannot be copied. [Allocation](../08-errors/02-allocation-and-traps.md)
and [destruction](06-destruction.md) describe failure and cleanup.

Only paths that continue contribute to availability at a branch join. Moving on
some continuing paths makes a binding possibly moved; moving on all makes it
moved. Reinitializing a mutable binding restores availability. Every loop
backedge, including `continue`, must preserve outer owners available at loop
entry. Moving and then reinitializing is accepted; carrying the move to a
backedge is rejected. Break paths join the zero-iteration path.

Owned fields cannot be moved directly out of a class or indexed collection.
Use an explicit copy, a collection removal operation, whole-tuple unpacking,
or consuming enum matching as appropriate. A class field can also be exchanged
with [`replace`](../04-expressions/assignment.md), transferring its old value
while leaving the field initialized.
