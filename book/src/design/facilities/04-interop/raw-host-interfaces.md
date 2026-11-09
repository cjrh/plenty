# Trusted raw host interfaces

A host ABI can retain raw ports, expose a descriptor of callback addresses, and
call an entry with overlapping buffers. Trusted `.plentyi` modules can describe
these interfaces directly. These operations are a low-level binding boundary;
they do not make arbitrary pointers safe Plenty values.

## Memory access

```text
pub opaque Pointer
pub extern def read_sample(p: Pointer, offset: u64) -> f32 = intrinsic "load"
pub extern def write_sample(p: Pointer, offset: u64, value: f32) -> () = intrinsic "store"
pub extern def advance(p: Pointer, offset: u64) -> Pointer = intrinsic "offset"
```

Offsets count bytes. `load` and `store` accept numeric scalars, `bool`, or opaque pointer
values, using precisely their target C size. `offset` returns the same nominal
pointer type. These lower to native loads, stores and address arithmetic; there
is no generated C helper, allocation, copy-back adapter or retained Plenty loan.

The binding author must establish readable/writable initialized storage as
appropriate, sufficient extent, target alignment, valid lifetime, compatible
representation, and no data races. Pointer arithmetic must remain within its
allocation (or one past for an address never dereferenced), without wrapping.
Null pointers may be passed around and tested, but never dereferenced.
A pointer may be retained only while the host keeps the allocation alive.

A boolean load reads exactly one byte, treating zero as `False` and any nonzero
byte as `True`. A boolean store writes exactly one byte, 0 or 1. Neither
operation touches adjacent bytes.

Raw access permits aliases. A load returns a copied scalar, and a store writes
one scalar: neither operation creates an ordinary `&T` or `&mut T`. Sequential
loads and stores observe preceding writes through aliases. Bindings must not
combine these operations with conflicting live Plenty references.
This leaves [ordinary borrowing](../../language/05-ownership/02-borrowing.md)
unchanged.

For an in-place stereo processor, load both input samples before writing either
output sample. This supports matching input/output buffers and cross-channel
aliases at the same frame. Partial overlap across frames still has the
algorithm's sequential read/write semantics; it does not promise a snapshot of
the original input. A wrapper needing snapshot semantics must reject that
arrangement or copy explicitly.

## Resident addresses

```text
pub extern def callback() -> Pointer = address "plugin_process"
pub extern def uri() -> Pointer = static "https://example.org/plugin"
pub extern def descriptor_storage() -> Pointer = storage 64
```

`address` returns the C entry adapter of an exported function in the same
library, not its private Plenty calling-convention address. The binding author
must match the callback signature expected by C. There is no implicit closure
capture or managed callback environment.

`static` returns immutable UTF-8 bytes with one appended NUL byte. `storage`
returns a stable, initially zeroed, 16-byte-aligned writable allocation whose
size is an unsuffixed literal from 1 through 1048576. Repeated calls return the
same allocation for that declaration; different declarations have independent
storage. Neither operation allocates on entry. Never free these pointers;
they remain valid only while the library stays loaded. Writes to static storage
need explicit host synchronization, including initialization.

For native protocols that discover data before calling any function, use
[initialized C data exports](exported-objects.md). Their pointer fields are
relocated by the native loader rather than populated by an initialization call.

## Export names and pointers

Numeric and opaque-pointer parameters and results can cross a C export directly.
Opaque pointers preserve their address and nullability; the generated `void *`
spelling does not specify ownership or grant permission to access the pointee.
Record the host's lifetime, ownership, aliasing, and access contract in the
export's docstring and binding documentation.

Ordinary exports retain their library-prefix requirement. An entry required by
an external protocol can explicitly select its exact symbol:

```text
export def descriptor(index: u32) -> Pointer = exact "lv2_descriptor":
    ...
```

Reserved compiler symbol namespaces and duplicate symbols remain rejected.
For entries called on host worker threads, use the checked
[`@thread_safe` export contract](host-threads.md). A trusted intrinsic declaration
can itself carry `@thread_safe`; this promises that callers provide independent
storage or synchronize accesses correctly. It does not make mutable global
storage safe to change concurrently.

## Current limits

This subset does not define C-layout Plenty classes, typed callback-pointer
values, C aggregate returns, automatic header import, or inferred raw-pointer
lifetimes. C record offsets and sizes must currently be specified by the binding
and verified against a native host. Named constants should isolate those layout
assumptions. Native targets currently have 64-bit pointers; this is not a
portable replacement for target-specific C layout calculation.

Generated import wrappers keep raw opaque types thread-bound conservatively.
Real-time execution guarantees and unloading remain outside this contract.
