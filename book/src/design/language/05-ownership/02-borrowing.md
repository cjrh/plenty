# Borrowing and reborrowing

`&place` creates a shared reference `&T`; `&mut place` creates an exclusive
reference `&mut T`. Exclusive access requires a mutable owner or an existing
exclusive reference. A shared loan permits other shared reads; an exclusive
loan prohibits competing reads, writes, and borrows. Neither acquires ownership.
`&mut str` permits replacing a string binding, not mutating string bytes.

A reference binding can be initialized from an explicit borrow, a named
reference, or a reference-returning call. Naming or passing an existing
reference reborrows it. An exclusive parent can lend shared or exclusive access;
conflicting parent access is prohibited while the child is live. A shared
reference cannot be upgraded to exclusive access. Reference bindings are
immutable unless declared `mut`; that declaration controls reassignment of the
reference, independently of the referent's shared or exclusive access mode.

Loans are checked through branches, loops, and early exits and normally end at
their last use. Moving, replacing, or destroying an owner conflicts with any
live loan into it. References cannot be stored in aggregates or retained across
generator suspension. Partial moves and general lifetime annotations are unsupported.

Class fields and fixed tuple indices are projected places. Disjoint fields can
be borrowed independently; access to an ancestor or whole owner overlaps its
descendants. `&items[index]` and `&mut items[index]` borrow list elements or
dictionary values in named storage, including projected collections. Indices
and keys follow ordinary indexing, including negative list indices and traps
for absent entries. Element loans at distinct nonnegative integer literal list
indices, such as `xs[0]` and `xs[1]`, are disjoint. Every other element loan
protects the whole collection: a variable, computed, or negative index, and
every dictionary key. Growth, removal, replacement, movement, and whole-list
access such as `len(xs)` or `&xs` overlap every element loan.
`&xs[a:b]` and `&mut xs[a:b]` borrow a run of list or byte-buffer elements as
a [slice view](../04-expressions/slices.md#slice-views), `&[T]` or `&mut [T]`.
A view's loan covers its run of elements; see
[disjoint slice views](#disjoint-slice-views) for when two runs are disjoint.
Nested fields and collection elements can be projected through these references.
Borrowing itself allocates nothing.

### Disjoint slice views

Two views of one owner are disjoint when the checker can prove their runs do
not overlap for every owner length. It accepts two forms:

- One view stops where the other starts, at the same integer literal or the
  same non-`mut` `i64` binding: `&mut xs[:m]` and `&mut xs[m:]`, or
  `&mut xs[:-2]` and `&mut xs[-2:]`.
- Both bounds compared are literals of the same sign, and one view stops at or
  before the other starts: `&mut xs[0:2]` and `&mut xs[4:6]`. An empty view
  such as `&xs[3:1]` overlaps nothing.

A list element at a literal index `k` is disjoint from a view that starts at a
nonnegative literal above `k` or stops at a nonnegative literal at or below
`k`. Nested views and rows of one view follow the same rules relative to that
view. Every other pair overlaps, including bounds of different signs, two
different names, a `mut` binding, and a computed expression such as `m + 1`.

The rule is sound because a view clamps each bound to the owner's length when
it is created, and the length cannot change while a view is live. A
nonnegative bound `v` becomes `min(v, len)` and a negative one `max(len + v, 0)`;
both are monotone, so equal or ordered bounds of one sign stay equal or ordered
after clamping. A shared bound name reads one value for both views: assigning
it while a view bounded by it is live is rejected.

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4, 5]?
    m = len(xs) // 2
    left = &mut xs[:m]
    right = &mut xs[m:]
    left[0] = right[0] + right[1]
    print(xs)?
    Ok(())
```

```output
[7, 2, 3, 4, 5]
```

```plenty-error
def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2, 3, 4, 5]?
    left = &mut xs[:3]
    right = &mut xs[2:]
    left[0] = right[0]
    Ok(())
```

```error
conflicting borrow
```

The diagnostic names both views and states the rule. Rewrite the bounds to
meet at one name, or end the first view before taking the second.

Distinct fields of one borrowed loop element are disjoint, as they are for any
other reference. Under `for p in &mut pairs`, `swap(&mut p[0], &mut p[1])`
borrows two fields of the current element; under `for n, s in &mut pairs`,
`n` and `s` are such fields. Borrowing the list or another element while they
are live still conflicts, because the loop element's index is unknown.

Integer node IDs do not borrow their target collection. They permit sequential
updates to different nodes, but do not make simultaneous indexed loans disjoint.
Use the [arena lesson](../../../tutorial/09-generics/arena-trees.md) for owned
storage with parent links and a traversal worklist.

An ordinary `&T` parameter implicitly borrows an owned argument: `inspect(value)`
and `inspect(&value)` lend the same owner. An `&mut T` parameter requires
`&mut value`, except when forwarding an existing exclusive reference. Methods
borrow their receivers according to their declared `self` parameter. Generic
calls and callbacks follow the same signature-based rules.

A class method with an exclusive receiver reserves its place before evaluating
arguments, then activates exclusive access just before entering the method.
The receiver and arguments still evaluate once, left to right. During argument
evaluation the reserved place permits shared reads, but cannot be moved,
replaced, or mutably borrowed. Thus `buffer.fill(0, buffer.len(), 0u8)` works
even when `buffer` is an `&mut Bytes`: the length is a completed value before
the exclusive call starts. Generic methods use the same rule. A shared
reference into the receiver that remains live as an argument still conflicts
with activation; a nested mutation of that receiver also conflicts. Bind a
nested mutation's result to a local before the outer call when sequential
mutations are intended. Explicit function arguments such as `f(&mut x, &x)`
do not receive this implicit receiver reservation.

Shared call arguments may be literals or owned temporaries. They evaluate once
in argument order and stay alive through later arguments and the containing
full expression, including cleanup on `?`. A reference returned into such a
temporary can be used within that expression, but cannot be bound, returned,
or retained by a scoped worker. Mutable temporary arguments and general
temporary lifetime extension are unsupported.
