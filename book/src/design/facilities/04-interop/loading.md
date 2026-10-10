# Runtime loading

A generated runtime interface exposes `load(path: &str) -> Result[Library, LoadError]`
and a Library method for each export. Importing it does not load native code;
the consumer needs no link argument for the loaded library. Generation currently
requires the original export source and differs from a linked `.plentyi`
interface. See [commands](../../toolchain/01-building/commands.md).

Methods preserve supported numeric/unit inputs and results, scalar borrows,
Result error types, and [owned class handles](owned-exports.md). Borrowed input
mutations remain visible on Err. The generated module is checked before publication.

Loading opens the requested library, compares exact discovery metadata, requires
the fingerprint guard, and resolves every export/destructor before returning a
usable table. Failed lookup never exposes a partial public table; its private
lookup lease closes on every path. Calls use cached native addresses.

`LoadError` has allocation-free `OutOfMemory`, `CapacityOverflow`, `InvalidPath`,
`OpenFailed`, `InvalidSymbol`, `MissingSymbol`, and `IncompatibleContract` variants.
Path termination requires one fallible Plenty allocation. The private lease
wrapper and returned method table are inline classes; they allocate nothing.
Native constructors and the OS loader may allocate independently. Cached scalar
calls and adapters need no allocation beyond effects of the native callee.

Generation accepts at most 128 functions including destructors, and 240 parameter
slots per method; consuming owners require additional temporary slots.
Method names beginning `_`, new, and self, and exported class names Library and
load, are reserved. Native symbols must be shorter than 512 bytes; discovery
metadata must fit 4 MiB.

## Owned objects and code residency

Returned class wrappers store their handle, originating identity, and exact
destructor address. Inline wrapper construction allocates nothing; the native
factory's owned-handle allocation remains fallible. Factory Err cleans the empty
wrapper. Consuming arguments transfer on both Ok and Err; successful class results
arm a new wrapper.

Objects may outlive the Library table. Successfully loaded code and native static
state remain resident until process exit, so their originating destructor remains
callable. Before any class argument crosses the native boundary, methods compare
its origin with their table. A different loaded instance traps with a diagnostic,
even if contracts match; it is not a LoadError. Repeated loads of the same
resident instance are compatible. Calls and objects stay on their creating thread.

A path containing `/` is absolute or relative to the process working directory;
bare names follow platform search rules. Native constructors can run during open
before a later check fails. Loading trusts native executable code; metadata is
compatibility checking, not authentication.
