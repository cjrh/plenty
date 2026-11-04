# Boxed protocol objects

`Box[P](value) -> Result[Box[P], AllocError]` consumes a concrete class, enum,
or `bytes` value satisfying protocol `P`. Different implementations have the
same owner type, so a `list[Box[P]]` or dictionary with `Box[P]` values can call
protocol methods without matching. Protocol arguments must be fixed, for
example `Box[Readable[i64]]`.

Construction makes one allocation, including for an empty payload. Failure
destroys the consumed input. A static method table is shared by objects with
the same concrete implementation and protocol. Calls, moves, and compiler
cleanup add no allocation; user methods and destructors retain their own costs.
Collection insertion retains its ordinary consumed-input failure contract.

Every method operates on the same concrete payload. Shared methods borrow it;
mutable methods require a mutable binding or exclusive reference. Loans remain
rooted in the owner, including through fields and collection elements. An active
borrow prevents conflicting replacement, removal, mutation, or movement.
Read-only calls on temporary owners keep them alive through the expression;
mutable calls require a place.

Only protocol methods are visible. `Box[P]` satisfies the identical `P` bound,
including its fixed arguments. Generic inference can use those arguments without
knowing the hidden concrete implementation. Structural matching against another
protocol, even one with identical methods, is unsupported. Conformance checks
required methods from the protocol's defining module; adapters grant no private
access.

## Boxing eligibility

Every requirement must have a shared or mutable receiver and fixed argument
and result types. Ordinary owned arguments, borrowed arguments and slice views,
unit results, aggregate results and fallible results are supported. Calls take
positional arguments. The complete protocol is checked when `Box[P]` is used.

These requirements prevent boxing, while remaining valid for static generic use:

- Method-local type parameters.
- A consuming receiver.
- The implementing type outside the receiver, including nested occurrences.
- Returned references or views, including references inside `Option` or `Result`.

Generator-frame and concrete-closure signature types remain unsupported by the
indirect-call ABI. Mutually recursive boxed interface signatures are diagnosed rather than expanded
indefinitely.

`std.fmt.Formattable` has a method-local writer parameter and cannot be boxed.
The constructor accepts specialized generic classes and enums, but does not
introduce new builtin conformances or forward arbitrary anonymous sums.

## Ownership and limits

Bare `P`, `list[P]`, and `&P` remain unsupported value types. Use `&Box[P]` to
borrow an erased owner. `Box(value)` still makes a concrete `Box[T]`; annotations
and arguments never implicitly erase or allocate. `Box[P](existing_box)` is
rejected: construct from the concrete owner directly, or move an existing
`Box[P]` without another constructor.

Protocol boxes can be stored in ordinary aggregates and returned from factories.
They cannot be copied, compared for equality, hashed, used as keys/set elements,
or extracted with `*box`. Downcasts and protocol conversions are unsupported.
Automatic formatting writes `<boxed protocol>` without inspecting the payload.
These restrictions apply through containing aggregates too.

Native worker/channel transfer is rejected, including through nested containers.
A protocol box cannot conceal executors, futures, channel endpoints, or erased callback captures;
these would hide ownership obligations from static checks. `Shared` owners
cannot contain protocol boxes, because their erased payloads could conceal
ownership cycles or owners whose destruction waits. Indirect calls retain the
compiler's conservative effect checks.

The channel restriction includes `Sender[T]` and `Receiver[T]` nested in classes,
enums or containers. Hiding an endpoint would prevent the compiler from checking
whether it can block a worker during a join, result wait or pool shutdown. Keep
such payloads in concrete owners such as `Box[T]`, and drop the relevant endpoint
holder before waiting.

See the [tutorial](../../../tutorial/09-generics/boxed-protocols.md),
[concrete boxes](../../facilities/01-builtins/boxes.md), and
[design and research](../../../proposals/completed/boxed-protocols.md).
