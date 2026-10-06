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
  and the high word stores the tag path for inline `Option`/`Result` wrappers
  and the builtin `AllocError` enum.
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
- Collection dispatcher opcodes 28 (reserve) and 29 (insert) return inline
  `Result[(), AllocError]`: zero on success, `1 << 64` for exhaustion, and
  `3 << 64` for capacity overflow. Inputs are borrowed on both outcomes; only
  committed entries retain payloads. Size validation precedes allocation, and
  both entry and hash buffers are reserved before changing logical contents.
  Failed `realloc` leaves the old buffer valid. Rust `Vec` keeps allocation and
  deallocation paired with the fixed global allocator; custom allocator
  selection is not exposed yet.
- Opcode 32 constructs an empty collection with the supplied initial capacity,
  returning `Result[collection, AllocError]`. Validate layouts and reserve buffers
  before allocating the owner header; Rust drops any temporary buffers on error.
  Successful construction moves the buffers into initialized raw header storage.
  The ordinary constructor shares this path. Collection destruction drops the
  buffers before freeing the header with the same checked allocation layout.
- Opcode 33 returns `Result[T, AllocError]` from a borrowed source value and
  its type descriptor. Deep copies reserve collection storage before copying
  entries; temporary owner guards reclaim pending values and partial collections.
  Records release only their initialized field prefix on failure and free the
  partial record without invoking its destructor. Immutable payloads are retained
  without allocation. Ordinary copying uses the same implementation with terminal
  error handling. Custom-cleanup values remain statically uncopyable.
- Opcodes 34 (text concat) and 35 (text join) borrow strings/a list of strings
  and return `Result[str, AllocError]`. A first pass checks combined byte/scalar
  lengths; the output uses one checked header/payload allocation. A second pass
  copies exact bytes without intermediate buffers or user callbacks. Empty and
  singleton joins also allocate an output. Ordinary concat uses the same helper
  with terminal failure handling. All paths preserve UTF-8 and embedded NUL bytes.
- Opcode 36 splits two borrowed strings and returns `Result[list[str], AllocError]`.
  The compiler supplies that result's immutable descriptor. Count literal,
  non-overlapping separator matches without allocating, reserve the list, and
  allocate each piece using checked string storage. An owner guard reclaims all
  completed pieces on failure; no source storage is consumed or retained by the
  result. Empty pieces are preserved and also allocate. Empty separators are
  invalid operations and trap before allocating, rather than returning `AllocError`.
- Opcode 37 borrows a string and an `i64` index, returning
  `Result[Option[str], AllocError]`. Signed index normalization is checked before
  scanning UTF-8. Missing indices return zero without allocating; present scalars
  use one checked string allocation and the inline `Ok(Some(...))` tag path.
  Allocation failure uses the ordinary inline error tags. Normal string indexing
  shares this helper and converts missing indices/allocation errors into traps.
- Opcode 38 performs dictionary lookup and returns inline `Option[V]`. The
  frontend permits only non-affine values. Hash lookup borrows the dictionary and
  key; a hit retains its value before wrapping it in `Some`, and a miss returns
  `Nothing`. Neither path allocates. A retained result survives entry replacement
  or dictionary destruction, including nested inline sums and immutable enums.
- Opcode 39 removes a dictionary entry through an exclusive receiver and returns
  inline `Option[V]`, transferring the payload owner without retain/release.
  The stored key is released after repairing the hash index. Remaining entries
  keep their order; both buffers retain capacity. Bucket rebuilding uses existing
  storage and no allocation, scanning the table and rehashing remaining keys. A miss
  leaves storage unchanged. Dropping the result owns payload cleanup, including
  user destructors; the runtime never runs a payload destructor during removal.
- Opcode 40 removes a list element and returns inline `Option[T]`. The compiler
  supplies `-1` for an omitted index; checked normalization handles negative and
  extreme indices without overflow. A hit transfers the entry's payload directly;
  a miss leaves the list unchanged. Ordered removal shifts later slots without
  allocating or shrinking the buffer. Last-element removal requires no shifting.
- Opcode 41 discards a set member and returns a Boolean indicating whether it
  was present. It shares dictionary removal's index rebuilding and capacity
  retention, releasing the stored member without consuming the borrowed query
  value. Sets have a zero entry payload; successful removal of that payload must
  still be distinguished from a missing key. Neither path allocates.
- Opcode 42 performs checked list lookup and returns inline `Option[T]`. Checked
  signed index normalization precedes the read; a hit retains its payload, and a
  miss returns `Nothing`. The compiler restricts this to non-affine elements.
  Neither path allocates or alters storage, and retained payloads outlive the list.
- Opcodes 43/44 implement dictionary `try_keys`/`try_values`, with output metadata
  `Result[list[T], AllocError]`. They reserve the complete list buffer and header
  before touching source owners, then retain immutable slots or transfer affine
  values and zero their old slots. The frontend permits affine values only from
  unique owned temporaries. No step after reservation can allocate or call user
  code. Failures leave the source untouched; compiler cleanup consumes temporary
  receivers on either outcome. Ordinary `values` (opcode 11) shares this helper
  with a terminal allocation-failure policy. Keys and values retain insertion order.
- Opcode 45 builds a fallible forward list slice from two signed bounds. Bounds
  normalize with wide arithmetic and clamp to the source length. Reserve the
  output buffer and header before retaining immutable elements or transferring
  selected affine slots from a unique temporary. Unselected slots remain for
  ordinary input cleanup. The compiler supplies the list type descriptor.
- Opcode 46 implements `str.try_slice` with the same clamped signed bounds as
  lists, measured in Unicode scalars. Character iteration locates a UTF-8 byte
  interval; only the final string is allocated, including empty/full slices.
  Both inputs and output use their normal independent ownership lifetimes.
- Opcode 47 implements literal `str.try_replace`. Count non-overlapping matches
  (empty patterns match scalar boundaries), check final byte/scalar lengths, then
  allocate one output and copy unmatched spans and replacements directly. Length
  arithmetic subtracts removed contents before adding replacements, and checks
  multiplication, addition, and layout overflow. No match-position buffer or
  intermediate text allocation is used, including no-match and empty results.
- Opcodes 48/49 compare literal string prefixes/suffixes without allocating or
  modifying any owner. Empty patterns match, including empty input.
- Opcodes 50/51 find the first/last literal substring and return inline
  `Option[i64]`. Convert the UTF-8 byte match offset to a scalar count without
  allocating; empty needles match at the corresponding end of the source.
- Opcode 52 counts non-overlapping literal matches with no allocation. Empty
  patterns count scalar boundaries; valid string sizes keep the result within i64.
- Opcodes 53–55 trim Unicode White_Space at both/left/right ends respectively.
  Borrow the input, locate the remaining byte interval, and allocate one final
  independent string; failures preserve the source.
- Opcode 56 repeats text using checked byte/scalar multiplication and one output
  allocation. Nonpositive counts produce empty strings. Copy the input once and
  double the initialized output prefix with disjoint copies; empty inputs never
  loop over the count. Layout overflow is detected before any allocator call.
- Opcode 57 reverses list entry slots in place without payload retain/release or
  allocation. The frontend requires exclusive access and discards the retained
  internal receiver result so the public method returns unit.
- Opcode 58 clears a list/dictionary/set: reset hash buckets, drain entries in
  source order, and release keys before values while retaining buffer capacity.
  Runtime clearing allocates nothing; user cleanup follows its own policy.
- Opcode 59 reserves destination list capacity before appending a consumed source's
  entry slots. It returns inline `Result[(), AllocError]`. No payload retain/copy
  is needed; source entries are empty on success and intact on failure, then the
  compiler releases the source owner on either outcome. The checker forbids aliasing
  source/destination owners and requires exclusive destination access.
- Opcode 60 consumes a dictionary into an exclusive destination. Count absent
  keys, reserve both buffers, then transfer entries without further allocation.
  Keep existing key positions/owners and release replaced payloads after installing
  new ones; append new keys in source order. On failure both runtime objects keep
  their contents, and generated cleanup releases the consumed source.
- Opcode 61 consumes a set into an exclusive destination, sharing dictionary
  update's missing-key counting and reserve-first transfer. Duplicate incoming
  member owners are released without changing existing owners. Sets have no
  value type, so the shared path never releases a dictionary payload for them.
- Other runtime failures terminate without unwinding Plenty frames. Internal Rust panics
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
Failure-injection markers reject all allocations after N successful calls, until
explicitly restored. They cover alloc, alloc_zeroed, and realloc using a constant
thread-local budget, without allocating inside the allocator. Integration tests
exercise every collection construction/growth/reservation allocation and nested
copy and string-splitting allocation, including partial-record, pending-key, and
partial-piece cleanup, and retry after
each failure. Raw ABI tests also run under `--features allocation-checks`.
Checked character lookup also verifies zero allocations for missing indices,
one for present scalars, and allocation-free propagation on failure.
Optional dictionary lookup checks allocation-free hits/misses, equal but separately
allocated string keys, and returned-value lifetime across update/destruction.
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
