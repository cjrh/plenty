# Rust

Plenty's ownership model is Rust-influenced: values have one owner, borrows
are checked, and destruction is deterministic. See
[ownership, borrowing, and lifetimes](../language/05-ownership/index.md).
This page records where the two languages differ.

## Unsafe code

Plenty has no `unsafe` blocks. Rust's `unsafe` permits five operations that
safe Rust rejects; Plenty code can perform none of them, except calling C
through trusted declarations.

| `unsafe` Rust permits | Plenty |
|---|---|
| Dereferencing raw pointers (`*const T`, `*mut T`), with pointer arithmetic, null or dangling pointers, and aliased `&mut` | Not permitted. `*` dereferences only checked references (`&T`, `&mut T`). The only raw pointers are [`opaque` foreign handles](../facilities/04-interop/trusted-declarations.md), which cannot be dereferenced, converted to integers or offset. |
| Calling `unsafe` functions: C functions, `transmute`, `get_unchecked`, manual allocation, uninitialized memory | C functions are declared in [`.plentyi` modules](../facilities/04-interop/trusted-declarations.md), whose author vouches for the C side. Bit reinterpretation is limited to four std-only integer/float intrinsics used by [`std.encoding`](../stdlib/encoding.md). The other operations do not exist. |
| Reading and writing mutable statics | Not permitted. There are [no global variables](../language/02-source/index.md); module scope holds only [constants](../language/02-source/modules.md#constants). |
| Implementing `unsafe` traits such as `Send` and `Sync` | Not permitted. [Worker eligibility](../facilities/03-concurrency/index.md) is a closed-world compiler check of stored types, reachable bodies and [native effects](../facilities/03-concurrency/effects.md); code cannot assert it. |
| Reading `union` fields | No unions. [Enums](../language/03-types/enums.md) and [anonymous sums](../language/03-types/anonymous-sums.md) are tagged and checked when matched. |

Rust's inline assembly (`asm!`) also requires `unsafe`. Plenty has no inline
assembly.

### Where unchecked code lives

Code that Rust would write inside `unsafe` lives outside Plenty source:

- trusted `.plentyi` declarations, where a wrong declaration or a faulty C
  library [can violate memory safety](../facilities/04-interop/index.md);
- the [Rust runtime](../toolchain/02-architecture/runtime.md), whose
  scoped `unsafe` code handles raw storage, packed pointers, counts and
  callbacks;
- compiler intrinsics used by the standard library.

A defect in one of these, or in the compiler, can corrupt memory. Plenty
code cannot.

### Consequences

- **Buffer overruns.** List, text, range and `Bytes` indexing is
  bounds-checked, and an out-of-range index stops the program with
  `index out of bounds: index I, length N`
  ([traps](../language/08-errors/02-allocation-and-traps.md)). No option
  disables these checks. [Slices](../language/04-expressions/slices.md)
  clamp their bounds instead of trapping. Integer overflow traps outside
  `checked:`, so an overflowed length cannot size a buffer.
- **References.** A reference is created from existing storage, such as a
  variable, a list element or a slice, and cannot outlive it. Creating one
  from an index goes through the bounds check.
- **Limits.** A custom allocator, an intrusive linked list, an unchecked fast
  path or memory-mapped device access cannot be written in Plenty. Write it
  in C or Rust and call it through [C interoperability](../facilities/04-interop/index.md),
  or add it to the runtime.
