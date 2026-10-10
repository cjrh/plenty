# C interoperability and libraries

Trusted `.plentyi` modules describe foreign ABI and lifetime promises. The
compiler checks declared types, ownership, and loans; binding authors and native
callers must supply matching implementations, valid storage, and obey the stated
aliasing rules. Foreign code can violate memory safety. No foreign unwinding or
nonlocal jump may cross Plenty frames.

- [Trusted declarations and ABI types](trusted-declarations.md).
- [Borrowed arguments and text adapters](borrowing-text.md).
- [Owning foreign resources](foreign-owners.md).
- [Exports and generated C contracts](exports.md), including
  [owned library objects](owned-exports.md).
- [Embedded contracts and compatibility](contracts.md).
- [Runtime loading](loading.md) and [trusted loading primitives](loading-primitives.md).

The supported ABI target is `x86_64-unknown-linux-gnu`. Calls and owned exported
objects remain on their creating caller thread, with serialized access as required
by the generated C contract. Ordinary `pub` controls Plenty imports and does not
export a native symbol. [Building and linking](../../toolchain/01-building/index.md)
is explicit; source imports alone neither link nor load a library.
