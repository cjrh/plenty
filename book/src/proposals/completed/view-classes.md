# View classes with one borrowed origin

**Status: Approved and implemented in local PR #1254, 2026-11-14.** This is
the step-2 decision requested by issue #945 and comment #2869. The
[view-class reference](../../design/language/05-ownership/view-classes.md) describes
implemented behavior. Its [qualification record](view-classes/implementation.md)
records implementation boundaries and the user-accepted explicit-initializer
contract for multiple shared fields. Generated constructors do not infer
relationships between independent borrowed inputs. The step-2 approval did
not include the separate iterator protocol; the subsequently authorized
[step-3 protocol](../../design/language/09-iteration/08-user-iterators.md) now
uses these records. Broader borrowing remains B11 in the [backlog](../../backlog.md).

## The decision

Allow an ordinary `class` to contain direct reference or builtin-view fields.
Such a declaration is a **view class**, recognized structurally; there is no
new annotation, lifetime syntax, or runtime borrow flag. It is an inline value
that carries one checked origin loan. It can name borrowed storage together
with ordinary state: a cursor position, matrix dimensions, or a SIMD group
count. Its borrowed fields are initialized once and never rebound.

Use a view class when several operations need the same borrowed data and
state. Keep a free function over a builtin view when there is no state to
name. Owning collections remain the way to keep data independently of its
source. A view class adds no authority to skip bounds or arithmetic checks.

The chosen first version is deliberately conservative:

| Question | Proposed answer |
| --- | --- |
| Origin | Every borrowed field derives from one constructor origin; all returned paths derive from the first loan-carrying parameter. |
| Shared fields | Several shared fields may borrow that same origin. An explicitly `@copy` view class copies only when all ordinary fields also copy. |
| Exclusive fields | Exactly one exclusive borrowed field, and no additional borrowed field. The instance is affine, never `@copy`. Multiple exclusive fields and mixed permissions are deferred. |
| Other fields | Ordinary currently storable types, with their existing ownership and cleanup. No nested view classes or borrowing frames. |
| Lifetime | Last use of the instance and every derived borrow; scoped workers extend it to their lexical join boundary. |
| Methods | A borrowed result borrows the named receiver for its complete use, including a shared receiver. No result outlives that receiver. |
| Cleanup | No user-defined `__del__`, `__enter__`, or `__exit__` on a view class. Ordinary owned fields still drop normally. |
| Standard sums | One `Option[ViewClass]` or `Result[ViewClass, E]` layer, success only. Full inline payload plus a separate discriminant; no length bits repurposed. |
| Storage | Local bindings, ordinary calls and scoped-thread inputs only; no heap storage, tuple/user-enum payload, ordinary class field, erased callable or detached job. |
| Representation | Existing inline class storage and private call ABI, with non-owning reference fields. No hidden allocation or promise that every field fits in registers. |

The one-exclusive-field restriction avoids adding aggregate splitting or
permission merging to this checker change. Multiple shared fields still cover
several projections of one source. A matrix or mutable cursor needs only one
exclusive data field. Supporting several independent origins would require a
separate design decision; this proposal does not conservatively borrow every
argument and call that a single origin.

## A proposed cursor

This illustrative cursor uses the class and method spelling selected by the
approved design; runnable examples live in the reference:

```text
class ByteCursor:
    _data: &[u8]
    _position: i64

    def __init__(self, data: &[u8]) -> ():
        self._data = data
        self._position = 0

    pub def remaining(self: &ByteCursor) -> &[u8]:
        &self._data[self._position:]
```

Construction borrows the source, without copying bytes. `remaining` keeps its
normal range check. Its result protects both the cursor local and the source;
the cursor cannot be moved, dropped, or mutably borrowed while that result is
used. A mutating cursor method can update `_position` through `&mut self`, but
cannot replace `_data`. An infallible constructor does not gain an allocation
error merely for holding borrowed fields.

This strict method rule is less permissive than returning a shared view tied
directly to the original data. It avoids lifetime inference through arbitrary
user methods. A consuming `self: ByteCursor` method may return another view
class derived from its carried origin, but may not return a reference to the
consumed cursor's own state. The detailed [contract](view-classes/contract.md)
distinguishes borrowed data from receiver storage.

## What approval would and would not establish

Approval selects these source rules and representation boundaries. The
implementation must still pass the ownership, negative-test and layout gates;
a discovered need for additional checker reasoning comes back as a design
question. Source constructors cannot certify an optimizer invariant. A matrix
constructor's size check can help only where existing analysis independently
proves the later access safe.

No `next` protocol, `for` desugaring for user classes, borrowed generator
storage, window/chunk iterator, SIMD reinterpretation, or automatic check
elimination is included. Issue #950 was independently delivered through builtin window/chunk loop
lowering; it does not depend on a user view-class iterator protocol. Existing
builtin views, generators and free `std.simd` functions retain their contracts.

Implementation must update the reference, relevant tutorial examples, cheat
sheet and delivery records together. The reference is authoritative for implemented contracts.

## Alternatives

- **Continue with free functions and separate cursor arguments.** This has
  the smallest checker surface, but cannot encapsulate borrowed data and its
  private state. It remains appropriate for stateless operations.
- **Named lifetimes or several inferred origins.** More expressive for
  disjoint fields and returned methods, but adds lifetime relationships beyond
  the first-origin model. Defer until a concrete case needs them.
- **Permit all aggregates to hold references.** This immediately reaches
  collections, recursive storage, erasure, cleanup and worker transfer. A
  restricted view class supplies the requested abstraction without opening
  those boundaries.
- **A reference-counted or generation-checked runtime handle.** This changes
  costs and turns forbidden source programs into runtime failures. Reject it
  for checked, allocation-free views.
- **Require an explicit `@view`.** It makes the category visible, but duplicates
  the reference-field declaration that determines the restrictions. Diagnostics
  should name the borrowed field and explain the category instead.

See the [ownership contract](view-classes/contract.md) for the complete boundary,
and [implementation and qualification](view-classes/implementation.md) for
source evidence and acceptance tests.
