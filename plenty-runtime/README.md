# Runtime contributor guide

`plenty-runtime` is the dependency-free Rust workspace crate behind generated
native code. The book's compiler/runtime architecture pages describe packaging,
representations, memory, and synchronization. This guide records boundary
obligations and validation for runtime contributors.

## Boundary invariants

- Helpers use the host C calling convention with fixed-width scalars and typed
  storage pointers. Rust `Vec` and enum layouts never cross that ABI.
- Generated code supplies initialized fields, bounded immutable descriptors,
  valid pointer lifetimes, and checked ownership transfers. Internal zero slots
  mean moved or uninitialized storage, never source-level nullable references.
- Borrowing helpers return owned results. Consuming helpers explicitly transfer
  messages, jobs, or results; count changes alone do not confer mutable access.
- Moves of inline payloads repair their embedded addresses. Inline frame cleanup
  completes before owner-provided storage expires. User callbacks and destructors
  run outside synchronization locks and outstanding mutable runtime views.
- Allocation uses checked layouts and guards for partial construction. Buffers
  retain matching allocation/deallocation provenance. Traps and Rust panics
  terminate rather than unwind through generated frames.
- Layout assertions and native integration tests protect compiler/runtime
  agreement. An internal layout is not a public foreign ABI.

## Validation

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
MIRIFLAGS=-Zmiri-permissive-provenance cargo +nightly miri test -p plenty-runtime
```

`cargo test -p plenty-runtime` tests the crate independently. Its Cargo library
omits the process entrypoint; embedded application builds enable it separately.

`runtime-checks` enables allocation counting and failure injection in the
compiler's archive. Allocation-free regions count every attempt, including
temporary allocations already freed. Failure injection covers allocation,
zeroed allocation, and reallocation without allocating inside the instrumentation.
Native tests check cleanup, retained source state, and retries after failure.
Normal compiler builds omit these checkpoints. The runtime crate's
`allocation-checks` feature enables its counting allocator and gated ABI unit
tests. `--all-features` selects both; workspace `--features runtime-checks` alone
does not enable the runtime crate's feature.

Miri checks layouts, metadata, ownership, relocation, and reentrant destruction.
Packed pointer slots require exposed-provenance semantics; this does not establish
strict-provenance correctness. Native tests remain necessary because Miri cannot
execute Cranelift-generated machine code.

For instrumented archives, `PLENTY_RUNTIME_RUSTFLAGS` supplies whitespace-separated
rustc flags. Compiler `RUSTFLAGS` are not copied implicitly. A nightly address
sanitizer build can use `-Zsanitizer=address -Clto=off` with an ASan-capable linker
wrapper. Record the flags and tested native boundary with validation results.
