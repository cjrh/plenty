# Plenty's native runtime

This dependency-free Rust crate implements native strings, collections, classes,
enums, generator frames, I/O, traps, and deterministic destruction. It uses Rust's
standard library; it is not a garbage collector or an interpreter. Plenty's
ownership and borrowing rules are checked before native code generation.

The compiler's `build.rs` compiles this crate with the selected `RUSTC` for Cargo's
`TARGET`, using optimization, ThinLTO, and abort-on-panic. The resulting static
archive and rustc's reported native link arguments are embedded in the compiler.
Direct rustc invocation avoids recursive Cargo builds and package-lock contention;
the crate intentionally has no external dependencies. Source changes rerun this
build step. Ordinary Plenty compilation extracts the archive and links it with
the Cranelift object through `cc`, without compiling runtime sources. The compiler
binary can be installed or moved independently of this repository and the Rust
toolchain. The archive is specific to the compiler's supported host target.

`cargo test -p plenty-runtime` tests the runtime independently. Its ordinary Cargo
library omits the native process entry point so Rust test harnesses can link it;
the compiler build enables `plenty_runtime_embedded` to supply `main`.

## Native interface and safety

- Exports use the host C calling convention, with fixed-width scalars and pointers.
  Rust collection and enum layouts do not cross this boundary.
- All object prefixes use `repr(C)`. Compile-time assertions preserve the layout
  expected by generated code: a 16-byte ownership header, string bytes and record
  fields at byte 32, and generator frame slots at byte 64.
- Runtime slots carry 128 raw bits: the low word is a scalar or pointer payload,
  and the high word stores the tag path for inline `Option`/`Result` wrappers.
  Floats use their IEEE bit patterns (`f32` in the low 32 bits), and unit payloads
  use zero. Aggregate calls pass aligned input/output slot pointers, avoiding a
  dependency on the platform's C ABI for 128-bit integers. Float equality follows IEEE
  rules even inside shared aggregates; memoized comparisons preserve linear
  traversal of shared payload graphs rather than expanding them into trees.
- Generated code supplies valid typed pointers, initialized fields, bounded type
  descriptors, and ownership transfers. Internal zero slots represent moved or
  uninitialized values; they are never source-level nullable references.
- Every helper borrows its operands and returns owned managed results, except
  explicit retain/release operations. Static string literals are immortal.
- Rust `Vec` owns collection buffers. Shared acyclic type metadata, names, and
  variant/field tables are emitted by the compiler as immutable program data;
  the runtime neither parses nor allocates metadata. Standard sum wrappers
  require no allocation, including during construction, projection, and propagation.
  Heap payload construction, explicit deep copies, and rendering can still allocate.
  Variable-size strings, records, and frames use checked allocation layouts and
  raw field addresses. Their destruction callbacks deallocate matching layouts.
- Destruction is queued iteratively, preserving child order. User drop hooks may
  reenter the runtime; no queue borrow or mutable Rust view of the dying record
  survives such a callback. The queue is thread-local; Plenty execution remains
  single-threaded and reference counts are non-atomic.
- Runtime failures terminate without unwinding Plenty frames. Internal Rust panics
  abort rather than crossing native frames.

## Validation

```sh
cargo test --workspace --features runtime-checks
cargo clippy --all --workspace --all-features
MIRIFLAGS=-Zmiri-permissive-provenance cargo +nightly miri test -p plenty-runtime
```

`runtime-checks` builds the compiler's archive with a counting Rust allocator.
After warming process-owned standard I/O buffers, it checks that program exit
returns to the baseline and that integration-test checkpoints keep live memory
bounded. Allocation-free regions additionally count every allocation attempt,
including temporary allocations freed before the region ends. A negative control
checks that the instrumentation detects a temporary string allocation.
This instrumentation and its checkpoint markers are absent from normal builds.

Miri covers raw layouts, flexible allocations, metadata sharing, float bit patterns
and unit payloads, deep generator destruction, field copies, and reentrant hooks.
The native ABI deliberately carries
pointer addresses in the low 64 bits of value slots, requiring exposed-provenance semantics; this
mode does not establish strict-provenance correctness.

For instrumented archive builds, `PLENTY_RUNTIME_RUSTFLAGS` supplies additional
whitespace-separated rustc flags. Compiler-only `RUSTFLAGS` are not implicitly
copied into the runtime build. For example, a nightly address-sanitized archive
can use `-Zsanitizer=address -Clto=off`, with an ASan-capable `cc` linker wrapper.
Native integration tests remain essential because Miri cannot execute
Cranelift-generated machine code.
