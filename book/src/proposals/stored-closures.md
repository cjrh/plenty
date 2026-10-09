# Stored closure environments

This records B32's storage decision. Current delivery status and priorities belong
in the [backlog](../backlog.md); the reference describes the implemented subset.

A registry needs an owner for callback state, a call signature, and a cleanup
contract. The existing generic class mechanism can express that owner when each
instance has one known environment type. Closure bodies already have concrete
identities and layouts. Preserve those identities when inferring a generic field
or collection element, and place the environment directly in that owner's slot.
There is no separate environment allocation and no dynamic dispatch. Constructing
or growing the containing heap object retains its ordinary fallible contract.

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

The native implementation needs type-sized extraction storage: the historical
range-sized scratch area cannot hold arbitrary environments. Reuse a removed row
in the collection's existing buffer instead of allocating during `pop` or remove.
Nested environments must relocate after buffer growth, shifting, reversal, and
transfer. Worker eligibility must inspect captures and callable bodies through
stored environments, including their destructors; storage does not certify thread
safety.
