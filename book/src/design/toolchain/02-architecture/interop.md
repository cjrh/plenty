# Native interfaces and metadata

Trusted interface declarations lower to host C calls, keeping Rust collection
and enum layouts behind private runtime helpers. Scalar output adapters use
call-scoped native storage; UTF-8 adapters lend bytes and length, and C-string
adapters create terminated temporary buffers. Owned exports use native boxes
with generated matching destruction. The
[interop contract](../../facilities/04-interop/index.md) defines what foreign
code must preserve; these adapters cannot enforce foreign behavior.

Generated export interfaces and headers describe names, ABI types, ownership,
and error conventions. Library artifacts embed interface metadata and a SHA-256
compatibility symbol. Bounded compile-time extraction reads metadata without
loading the binary; explicit verification compares its declared contract.

Typed runtime loaders cache resolved symbol addresses and check exact metadata
and object-instance identity. Runtime primitives use eager local GNU/Linux
loading and `RTLD_NODELETE`. Closing a lookup lease releases lookup ownership
while resolved code/static state remain resident until process exit. Dynamic
loading executes library constructors; it is not sandboxed metadata inspection.
Loader-owned memory and constructors are outside Plenty's allocator accounting.
