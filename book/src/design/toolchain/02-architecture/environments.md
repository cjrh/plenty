# Generator frames and closure environments

Each generator producer/specialization has a concrete frame layout, resume
function, and immutable descriptor. A frame owns parameters and locals in typed
slots, plus slot metadata, continuation state, resume callback, and reentrancy guard.
Its internal resume ABI is `(frame, out_slot) -> ready`; yields transfer owned
values, completion clears slots, and destruction releases remaining captures.

Ordinary slots use 16 bytes; inline payloads add their complete storage. Standard
sums reserve the largest possible payload, and frames can contain other frames.
Constructors and returning functions use caller-owned output storage. Moving
nested frames repairs addresses; cleanup completes before surrounding storage expires.

Layout caches are per concrete producer. Recursive inline layouts are rejected,
nesting is limited to 64 levels, native offsets fit signed 32-bit sizes, and
consumer specialization is limited to 256 per compilation. Unresolved factory
returns are rechecked with inferred context so all paths agree on concrete storage.

`Yield` requires an empty residual operand stack. Lowering adds resume-dispatch
edges to continuation blocks; all locals are frame-backed, so SSA values need
not survive invocations. Dispatch currently uses a linear comparison chain.
There is no interpreter, native-stack suspension, or eager yield collection.
Iteration wraps resume results in inline `Option` values.

## Closures

Capture environments are concrete inline layouts with nominal producer identities.
Consumers specialize for their actual layout; matching signatures do not erase
identity or introduce boxing. Owned factories return into caller storage. Nested
environment moves repair addresses, including environments stored in generator
frames or collection rows.

Reusable invocation borrows an environment; consuming invocation transfers it.
Borrowed capture loans remain source-level facts through environment moves and
generic consumers. Captures are released in reverse capture order; an
environment whose captures need no cleanup, such as a capture-less error mapper,
is not retained or released at all. Cached facts
and visited-type traversal avoid repeatedly expanding shared capture/type graphs.
Nesting is limited to 64 levels and storage must fit the native stack-layout limit.
The [callable contract](../../language/06-functions/index.md) defines permitted
captures, storage, and invocation; the [generator contract](../../language/09-iteration/index.md)
defines suspension and lifetime restrictions.
