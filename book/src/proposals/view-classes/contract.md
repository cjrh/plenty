# View-class ownership contract

**Not implemented; part of the proposed [single-origin view-class design](../view-classes.md).**
Examples are hypothetical. The [current ownership reference](../../design/language/05-ownership/index.md)
continues to govern implemented programs.

## Declaration and construction

A class is a view class when at least one direct field has type `&T`,
`&mut T`, `&[T]`, `&mut [T]`, or `&strview`. The referenced `T` is an ordinary
non-reference type. Plain fields use existing storable types. Another view
class, a reference-carrying sum, borrowing closure or generator cannot be a
field, even in another view class. This avoids recursive carried-loan layouts.

All borrowed fields must have shared permission, or there must be exactly one
borrowed field with exclusive permission. Multiple shared projections may
alias. No two-origin constructor is legal, even if the two owners happen to
have equal values. When fields originate from arguments that reborrow the
same checked origin, the checker must establish that relationship; an unknown
relationship is rejected.

For an explicit initializer, the first loan-carrying parameter after `self`
is the origin. Each borrowed field assignment must derive from it. `self`
names the destination under initialization, not its borrowed-data origin.
A generated fieldwise constructor takes the first borrowed field argument as
its origin and checks every later borrowed argument against it. Other
reference parameters may be read during construction but may not be retained.
Arguments evaluate once in written order, including named arguments, before
construction completes.

An initializer must definitely initialize each borrowed field exactly once on
every successful path. Assignment is legal only to that field of its own
uninitialized `self`; it is not legal after reading the field or through a
helper alias. A failed initializer cannot expose a partial instance. Its
already initialized owned fields drop once, and its argument loans end unless
an independently live value still holds them. Default expressions may not
introduce an implicit borrowed origin: borrowed fields require explicit
constructor inputs.

## Using a value

A local instance holds its origin until its last use and the last use of any
value derived from it. Moving the instance transfers the loan, not the source
storage. Explicitly dropping an unused instance releases that holder. Copying
a shared `@copy` instance creates another holder, so dropping one copy does
not release a still-used copy. Moving or resizing the source while any holder
is live remains a compile-time error. Inline strings retain their actual
source-slot stability; copying a descriptor cannot keep a temporary slot alive.

An exclusive view class moves on ordinary by-value use. A reference parameter
reborrows it: shared `&View` permits reading its data but cannot extract an
exclusive reference, while `&mut View` permits exclusive reborrows. Plain
mutable state is changed through `&mut self` or another existing legal mutable
place. Borrowed fields cannot be assigned, swapped, moved out, or replaced
through any place or destructuring pattern after construction. Reading one
produces a checked reborrow; it never copies an exclusive capability.

`mut view` reassignment follows the existing same-origin holder rule:
replacement may change plain state and the selected subrange but cannot
switch to another owner. The old value's owned fields are dropped normally.
A new origin requires a new binding. Initializing an empty optional holder
before assigning a view must not manufacture an untracked origin; support it
only with the existing carried-reference holder proof, otherwise reject it
with a diagnostic recommending initialization from the actual origin.

Ordinary owned fields retain their normal cleanup rules. The restriction on
view-class lifecycle hooks prevents a hidden scope-end read of borrowed data
from extending loans beyond apparent last use. Serialization, generated
copying into an owning container, user hash/key use, and native exports are
rejected for the loan-carrying class; redacted or diagnostic formatting may
borrow it transiently under its existing loan.

## Calls, returns and methods

A named view class counts as a loan-carrying input. A function may take it by
value, `&View` or `&mut View`; function generics can specialize to a concrete
view class only when all operations preserve its storage barrier and loan.
A type parameter still cannot bind directly to a reference. Merely hiding the
class behind `T` cannot admit `list[T]`, `Box[T]` or an erased callback.

A function returning a view class must return a value whose carried origin is
the first loan-carrying parameter. A by-value view parameter can forward that
carried origin when it moves into the result. A reference to a view parameter
has two relevant facts: its descriptor/state is borrowed for the call, and
its data already has an origin. Neither may be dropped accidentally when
forming or forwarding the result.

Methods returning `&T`, a builtin view, another view class or a supported sum
borrow their named receiver for the entire result lifetime. This strict rule
applies to shared and exclusive receivers. It protects a reference to plain
state and a reference into borrowed data uniformly. Another mutating method,
receiver reassignment or receiver destruction conflicts while that result is
live. A consuming receiver can transfer a view-class value's carried origin,
but cannot return an address into the consumed value's own record or fields.
Returning a copyable plain field is an ordinary owned result.

Named storage may supply constructors and methods. Borrowed results derived
from temporary owners or temporary receiver records may be used only within
the already supported full-expression lifetime. They cannot be bound or
returned. Where lowering cannot preserve the existing rule, reject the form
rather than extending a temporary's lifetime. Conditional choices keep all
possible dependencies without pretending they have one origin; a result with
incompatible origins is rejected.

## Optional and fallible construction

Permit one standard success wrapper: `Option[V]` or `Result[V, E]` where `V`
is a view class. Fallible `__init__` retains its existing `Result[(), E]`
declaration and produces `Result[V, E]`. `Nothing` and `Err` carry no view
loan; matching, propagation and unwrap move or copy a successful instance
with its loan. `Err` cannot carry a view class or reference.

The sum uses the existing full inline class payload plus its own tag. It must
not consume high bits of a borrowed field's pointer or length. Nested wrappers,
user enums, anonymous sums and tuples carrying view classes stay rejected in
this first version. Plain `Option[&T]`, returned builtin views, and their
existing wrapper rules do not change. Borrowing `&Option[V]` or an exclusive
sum holder is also deferred; match the value directly and borrow the extracted
view class. Shared copies and exclusive payload moves retain their respective
rules through every match arm.

## Storage, suspension and workers

Reject view-class values in collections, `Box`, ordinary class fields,
tuples, user enums, owned shared handles, channel messages, actor nodes and
erased or heap-stored callable environments. The same check applies through
aliases, generic instantiation and standard wrappers. Capturing the instance
by value is deferred; a local reusable closure may borrow a named instance
under the existing borrowed-capture rules, without returning a loan-carrying
result from a capturing closure.

A scoped native thread may receive the view by value or reference when the
ordinary worker eligibility checks accept its referent and plain fields.
All carried origin loans and receiver loans must be visible to the parent and
last until the lexical task boundary, even after explicit join. Exclusive
views cannot enter two concurrent tasks. Borrowed worker inputs keep the
existing job-free start-failure path; no start error may return the borrowed
instance as an owning job. View classes cannot be worker results, non-scoped
pool inputs, owned jobs or transferred actor data.

A view-class value may not be retained across generator `yield`, yielded as
an item, stored in a generator parameter/frame, or returned by a pipeline
adapter in this version. Borrowing generators supply useful checker
precedent, but extending their frame and item protocols is not implicit
approval of issue #945 step 3. This restriction keeps the new carried aggregate
out of suspension until its ordinary-function boundary is qualified.
