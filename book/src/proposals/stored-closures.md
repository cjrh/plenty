# Stored closure environments

> **Status: Implemented.** This records B32's storage decision.
> [Consuming and stored closures](../design/language/06-functions/04-consuming-and-stored-closures.md)
> owns the contract and
> [environments](../design/toolchain/02-architecture/environments.md) the
> layout.
>
> **Shipped differently:** classes, tuples, and enums are inline, so only a
> collection owner allocates. Environments that contain generators cannot enter
> heap storage.
>
> **Remaining:** heterogeneous erased environments and generator-containing
> environments in heap owners (B32); escaping borrowed captures (B11).

A registry needs an owner for callback state, a call signature, and a cleanup
contract. The existing generic class mechanism can express that owner when each
instance has one known environment type. Closure bodies already have concrete
identities and layouts. Preserve those identities when inferring a generic field
or collection element, and place the environment directly in that owner's slot.
There is no separate environment allocation and no dynamic dispatch. Growing a
containing collection retains its ordinary fallible contract.

Two calls to the same closure factory produce compatible environments. Different
closure expressions remain different types, even with identical signatures. A
finite family of environments can use an ordinary enum. A registry whose members
must have arbitrary unrelated environments needs a separate erased representation;
silently changing `Callable` would hide allocation and alter its thin-function ABI.

Owned boxing would give heterogeneous registries a fixed-size element but require
fallible allocation, a call/drop table, and an explicit ownership-transfer contract
on failed construction. Bounded inline erasure avoids that allocation but exposes
a size/alignment budget, rejects oversized environments, and reserves that budget
for every entry. Neither cost is justified for a homogeneous registry. Keep both
as alternatives for a demonstrated heterogeneous-storage use case.

Stored environments must own their captures. Borrowed captures cannot escape into
heap storage until stored-reference relationships are specified. Invocation follows
the existing reusable or consuming contract: reusable mutable callbacks require an
exclusive loan; one-shot callbacks must be extracted before invocation. Moving a
container relocates inline environment addresses without cloning captured owners.
Extraction and destruction must work with allocation disabled. Copy, equality, and
hashing remain unavailable for environments, including through containing values.

The native implementation uses type-sized extraction storage, because the
historical range-sized scratch area could not hold arbitrary environments. It
reuses a removed row in the collection's existing buffer instead of allocating
during `pop` or remove.
Nested environments must relocate after buffer growth, shifting, reversal, and
transfer. Worker eligibility must inspect captures and callable bodies through
stored environments, including their destructors; storage does not certify thread
safety.
