# Boxed protocol object contract

**Implementation note:** the [current reference](../../../design/language/07-generics/boxed-protocols.md) describes delivered behavior. This page preserves the original design and qualification plan; research-era implementation statements below are historical.
This is the proposed public contract for [boxed protocols](../boxed-protocols.md).
It is not implemented. All new syntax in this page is illustrative.

## Identity and construction

`Box[P[Args...]]` is an affine owner of one concrete value satisfying `P[Args...]`.
Protocol identity includes its resolved declaration and fixed type arguments;
import aliases do not create new identities. Distinct protocol declarations
remain distinct boxed types even when their signatures match.

`Box[P](../../boxed-protocols/value)` consumes an owned concrete value, verifies conformance, and
returns `Result[Box[P], AllocError]`. It attempts one allocation for the box,
including for a zero-field class or payload-free enum. Method tables are
static compiler output, not a second allocation per instance. Construction of
the input and operations within its methods retain their own allocation costs.

Allocation failure destroys the consumed value, as ordinary `Box(value)` does.
If the box is constructed successfully but insertion into a collection fails,
the collection operation's existing ownership contract applies to that box.
The proposal must not promise that a failed insertion returns its argument.
Programs preserving existing state should allocate detached replacement values
before moving state into them, as taught in
[recursive insertion](../../../tutorial/09-generics/recursive-insertion.md).

There is no implicit concrete-to-erased conversion on assignment, argument
passing, return, or collection insertion. `Box[P](../../boxed-protocols/value)` is the explicit
boundary. Allocation-free conversion from an existing `Box[T]` is outside the
initial scope: do not accidentally unbox/rebox it via today's expected-type
coercions. Initially reject boxed inputs to this constructor and suggest
boxing the concrete owner directly. Moving an existing `Box[P]` needs no
constructor and performs no allocation.

The admitted concrete sources are ordinary storage-safe classes and enums,
including specialized generic instances, and the existing `bytes` conformance
case. Do not introduce new builtin conformances as part of boxing. Existing
storage barriers still reject borrowed fields/captures, generators and other
values that cannot enter a heap owner. An arbitrary anonymous sum does not gain
protocol forwarding merely because it is boxed.

## Eligible protocols

A protocol is eligible when every requirement can be called with a fixed
signature after substituting its protocol arguments and erasing its receiver.
Check eligibility at a boxed-type use, not at protocol declaration: a protocol
that is ineligible for boxing remains valid for static generic use.

| Requirement | Initial rule and reason |
| --- | --- |
| `def hey(self) -> i64` | Supported; bare `self` is a shared borrow. |
| `def set(self: &mut P, value: i64) -> ()` | Supported; the object must be exclusively borrowed. |
| Ordinary owned and borrowed arguments, including slice views | Supported under existing call and storage rules; arguments are not retained by erasure. |
| Concrete owned results, including `Result[T, E]` | Supported; exact fallibility and payload types are part of the signature. |
| Protocol-level parameters, such as `Readable[i64]` | Supported once every argument is fixed; they determine one concrete table shape. |
| Method-level parameters, such as `put[T](../../boxed-protocols/...)` | Rejected; an ordinary finite table cannot supply every future specialization. |
| Protocol name outside the receiver, including nested occurrences | Rejected; that name denotes the hidden implementing type under current protocol rules. |
| Owned receiver `self: P` | Rejected initially; consuming an erased payload requires a distinct transfer/cleanup contract. |
| Returned references or views, including `Option[&T]` and `Result[&T, E]` | Rejected initially; dispatch must retain the origin loan before these are admitted. |

For example, `compare(self, other: &P)` requires another instance of the same
hidden concrete type. Two `Box[P]` values need not contain the same type, so
replacing that parameter with another protocol box would change the contract.
Similarly, returning `P` means returning the concrete implementing type today;
it must not silently become an allocating boxed result. Detect these uses
through aliases and nested signatures, before replacing the receiver type.

The current `std.fmt.Formattable` protocol has a method-local writer parameter.
It is therefore ineligible for this initial boxed representation. An erased
writer interface or a different reporting contract would be a separate design;
this proposal does not automatically create a general boxed error type.

Reject the entire boxed protocol if any requirement is ineligible; do not make
the callable method set depend on which methods a particular program uses.
Diagnostics should identify the protocol, offending requirement, and reason.

## Calls, mutation and loans

`object.method(args)` selects a method declared by the boxed protocol. The
compiler knows its signature; only the implementation address varies at runtime.
Concrete fields and additional inherent methods are not accessible through the
box. No runtime name lookup or conformance check is needed on a call.

Shared calls borrow the owner, including through `&Box[P]`, a list element,
a dictionary value or a class field. Mutable calls require `&mut Box[P]` or
an exclusively accessible mutable owner. Multiple methods share one payload:

```text
protocol Counter:
    def increment(self: &mut Counter) -> ():
        pass

    def current(self) -> i64:
        pass

def advance(items: &mut list[Box[Counter]]) -> Result[(), IoError]:
    for item in items:
        item.increment()
        print(item.current())?
    Ok(())
```

The read observes the increment made to that same instance. This must not be
implemented as separate independently captured copies for each method.

Loans remain rooted in the owner and its collection/field projection. A live
borrow prevents conflicting mutation, replacement, removal, moving or dropping
the owner, and invalidating collection changes. Preserve ordinary argument
evaluation and receiver reservation rules: erasure grants no extra aliasing.
Initially make no field-precision claim across indirect calls.

Read-only calls on temporary owners retain them until the full expression
ends. Mutable calls follow existing boxed-receiver place requirements. Calls,
owner moves, and compiler-provided cleanup add no allocation; user methods and
destructor hooks retain their own behavior and effects.

## Integration with generic consumers

Recommend that `Box[P]` satisfy the same `P` bound through its exposed methods:

```text
def greet[T: MyProtocol](value: &T) -> i64:
    value.hey()

def use(value: &Box[MyProtocol]) -> i64:
    greet(value)
```

Here `T` is `Box[MyProtocol]`; specialization compiles a forwarding indirect
call, not a new specialization for each hidden payload. Protocol arguments
come from the box's identity, including when generic inference needs them.
Moving a box into a generic owner parameter still transfers its owner normally.

This is deliberately restricted to the identical resolved protocol and arguments
initially. Matching another protocol from an exposed method subset, combining
protocols, and converting between erased protocols need separate rules. Do not
discover extra capabilities by examining the hidden payload.

Current generic bodies are checked per concrete instance. A body that relies on
extra inherent methods may work for a concrete class yet fail for `Box[P]`,
which exposes only `P`. Supporting boxed objects does not require broadening
that visible interface or silently implementing abstract generic-body checking.

## Visibility and other operations

Use the existing protocol conformance visibility boundary, with the protocol's
defining module as the bound site. Generated adapters must not gain private
access merely because they are emitted beside a concrete method. Public erased
signatures must expose accessible protocols and signature types. A library may
hide its concrete implementation behind a public `Box[P]` without exposing that
implementation's fields or type name.

Support owners in lists, dictionary values, class fields, enum payloads, tuples,
`Option`, `Result`, parameters and factory returns, subject to ordinary aggregate
restrictions. Dictionary reading must preserve ownership: use borrowing/index
operations supported for affine values rather than implying `get` copies a box.

Do not support extraction of an erased payload with `*object`, bare `&P` or
`&mut P` values, downcasts, type inspection, implicit copying, `copy`, equality,
equality-based membership, hashing, or use as keys/set elements initially.
Existing `Box[T]` behavior remains available for concrete `T`. Proposed automatic
formatting is an opaque marker such as `<boxed protocol MyProtocol>`; it must
not inspect private fields or invoke arbitrary formatting methods implicitly.

Reject native worker/channel transfer, including through nested containers.
Erasure hides both payload eligibility and indirect method/destructor effects.
Also apply conservative shared-owner cycle checks: mutable storage of an erased
object must not conceal a back-reference that existing `Shared` checks would
reject. Actor/task handoff and allocator provenance need explicit qualification;
support for local calls does not certify them.
