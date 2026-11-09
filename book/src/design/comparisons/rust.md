# Rust

Plenty's ownership model is Rust-influenced: values have one owner, borrows
are checked, and destruction is deterministic. See
[ownership, borrowing, and lifetimes](../language/05-ownership/index.md).
This page records where the two languages differ.

## Unsafe code

Plenty has no `unsafe` blocks. Rust's `unsafe` permits five operations that
safe Rust rejects. Plenty exposes selected native operations through trusted
C declarations and raw host intrinsics, while ordinary references remain checked.

| `unsafe` Rust permits | Plenty |
|---|---|
| Dereferencing raw pointers (`*const T`, `*mut T`), subject to validity and aliasing rules | `*` dereferences only checked references (`&T`, `&mut T`). Trusted [raw host interfaces](../facilities/04-interop/raw-host-interfaces.md) can load, store and offset opaque pointers, with validity, bounds, lifetime and synchronization obligations on the binding author. These operations do not create aliased `&mut` references. |
| Calling `unsafe` functions: C functions, `transmute`, `get_unchecked`, manual allocation, uninitialized memory | C functions are declared in [`.plentyi` modules](../facilities/04-interop/trusted-declarations.md), whose author vouches for the native contract. Raw host loads/stores can reinterpret native storage, and C allocator bindings can manage it manually. There is no general `transmute` or `get_unchecked` API. |
| Reading and writing mutable statics | Ordinary module scope has [no global variables](../language/02-source/index.md). Trusted raw interfaces can declare resident writable storage; its initialization and accesses require explicit synchronization. |
| Implementing `unsafe` traits such as `Send` and `Sync` | [Worker eligibility](../facilities/03-concurrency/index.md) checks stored types, reachable bodies and [native effects](../facilities/03-concurrency/effects.md). Trusted foreign declarations supply opaque-type mobility and C-call thread promises. [`@thread_safe` exports](../facilities/04-interop/host-threads.md) check reachable Plenty effects while relying on those promises. |
| Reading `union` fields | No unions. [Enums](../language/03-types/enums.md) and [anonymous sums](../language/03-types/anonymous-sums.md) are tagged and checked when matched. |

Rust's inline assembly (`asm!`) also requires `unsafe`. Plenty has no inline
assembly.

### Where unchecked code lives

Unchecked native operations are concentrated in these boundaries:

- trusted `.plentyi` declarations and their callers, where incorrect C contracts
  or raw-pointer use [can violate memory safety](../facilities/04-interop/index.md);
- the [Rust runtime](../toolchain/02-architecture/runtime.md), whose
  scoped `unsafe` code handles raw storage, packed pointers, counts and
  callbacks;
- compiler intrinsics used by the standard library.

A defect in one of these, or in the compiler, can corrupt memory. Declaring
a raw operation in a trusted interface does not prove that its Plenty callers
supply valid addresses or obey its lifetime and aliasing contract.

### Consequences

- **Buffer overruns.** List, text, range and `Bytes` indexing is
  bounds-checked, and an out-of-range index stops the program with
  `index out of bounds: index I, length N`
  ([traps](../language/08-errors/02-allocation-and-traps.md)) by default.
  [Unchecked assume mode](../language/03-types/numbers.md#unchecked-blocks)
  can omit checks for asserted preconditions; raw host accesses have no bounds
  checks. [Slices](../language/04-expressions/slices.md)
  clamp their bounds instead of trapping. Ordinary integer arithmetic reports
  declared errors; `unchecked trap:` traps on overflow, while unchecked assume
  mode places the arithmetic preconditions on the author.
- **References.** A reference is created from existing storage, such as a
  variable, a list element or a slice, and cannot outlive it. Creating one
  from an index checks bounds unless the author explicitly assumes that
  precondition in unchecked assume mode.
- **Limits.** Raw host interfaces permit manual native-memory algorithms, but
  do not provide C-layout classes, inferred pointer lifetimes, volatile device
  access or a safe allocator abstraction. Keep such code behind a small trusted
  binding and document its native contracts; ordinary collections and references
  remain the recommended interface for application code.
