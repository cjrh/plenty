# Rust-source semantic and interoperability contract

This is the proposed contract for the [Rust-source backend](../rust-source-backend.md),
not implemented behavior. Preserve the language first; make unsupported slices
explicit rather than accepting a program with different semantics.

## Values and API shape

| Plenty concept | Candidate Rust representation and constraint |
| --- | --- |
| Sized numeric scalars and bool | Corresponding Rust scalar; emit explicit checked operations/casts where required. |
| Simple scalar-field classes | Named Rust structs, normally move-only even when fields are Copy. Do not derive traits that weaken Plenty's ownership contract automatically. |
| Named enums, Option, Result | Native enums; explicit conversions for Plenty error widening and pattern tests. Stable public wrapper names for anonymous sums. |
| Shared/exclusive references | Rust references with generated lifetimes from checked place relationships. A returned first-parameter borrow gets that parameter's lifetime. |
| Borrowed primitive list data | `&[T]` / `&mut [T]` at selected Rust-facing boundaries; prove owner/lifetime stability and indexing behavior. |
| Owned collections | Initially preserve a support-crate wrapper; optional Vec conversion must state ownership, allocation failure and whether it copies. |
| Plenty immutable strings | Preserve share/retain behavior in a wrapper and borrow UTF-8 as `&str` where valid. `String` is not a transparent replacement. |
| Arbitrary Rust owners | Native Rust values only for explicitly admitted APIs with known loan/drop/effect contracts. Rust generic/trait declarations require more than an ABI signature. |

Do not transmute current [runtime values](../../design/toolchain/02-architecture/values.md)
to native Rust enums or collections. Plenty's sum tags, owner handles, string
headers and inline relocation rules are private representations. A source
backend may choose different internal layouts, but std/runtime helpers must
agree and every crossing must be checked. Initially isolate portable helpers
behind a support crate rather than copying the whole runtime or linking two
independent runtime/allocator graphs.

## Ownership and destruction

Ordinary lexical locals and many `?` exits fit Rust RAII. The probes validate
one such sequence, not the full contract. Plenty specifies closure capture
cleanup order and allocation-free bounded-stack recursive heap destruction;
ordinary Rust captures and recursively nested owners do not automatically
provide those guarantees. Rust's
[destructor rules](https://doc.rust-lang.org/reference/destructors.html)
also make declaration order, temporaries and partial moves consequential.
Generate explicit capture structs/scopes and cleanup helpers when necessary.
Avoid blanket `Option<T>` slots, hidden clones, `Rc<RefCell<T>>` or heap boxes
as a convenient way to appease the borrow checker.

Checked place relationships must survive lowering. A concrete example is
Plenty's simultaneous loans at distinct constant list indices: direct Rust
index borrows fail with E0499. A
[`split_at_mut`](https://doc.rust-lang.org/std/primitive.slice.html#method.split_at_mut)
translation works for the tested pair. General lowering must preserve argument
evaluation, bounds failure sites, intervening effects and loan scopes; the
two-index sketch is not a general algorithm. If unavoidable unsafe helpers are
introduced, centralize and audit their place/aliasing preconditions. Do not
emit pervasive raw pointers and call the result safe Rust interoperability.

Rust and Plenty impose different limits on stored/returned references. Continue
to enforce Plenty's current limits even when Rust could represent more. Conversely,
a backend must either translate an admitted Plenty construct or diagnose its
unsupported backend subset before emitting code.

## Arithmetic, errors and allocation

Plain Rust `+` in release mode is not the implementation of Plenty checked
addition. Emit `checked_add` plus the Plenty fatal trap path, and analogous
handling for other arithmetic, division, shifts and narrowing casts. Preserve
floating-point comparison and operation order; an optimization backend does not
authorize reassociation. Trap helpers need Plenty source/caller locations and
must not allocate on their failure path.

Native `Result<T, E>` is a good representation, but Plenty's automatic error-sum
widening needs generated mappings rather than accidental `From` implementations
or boxing everything into `dyn Error`. Public error enums should implement the
appropriate Rust traits and retain concrete causes. Rust panics are not
recoverable Plenty error variants by default.

Fallible construction must reserve before evaluating list elements, propagate
failure and destroy only initialized values. Rust's
[`Vec::try_reserve`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.try_reserve)
can implement one part, but `Vec::new()` does not perform Plenty's empty-owner
allocation. `vec!`, implicit cloning and unconstrained collection operations
can introduce infallible allocation. Preserve `CapacityOverflow` versus
`OutOfMemory` with explicit layout validation and an appropriate allocation
wrapper; simply exposing `TryReserveError` is not equivalent. External Rust
crates may allocate infallibly internally, regardless of generated code quality.
Their contract must say so; do not claim arbitrary Rust APIs satisfy Plenty's
recoverable-allocation policy.

## Panics, recursion and suspension

Plenty [fatal traps](../../design/language/05-ownership/06-destruction.md) do not
unwind. The probe's default Rust panic runs a destructor that Plenty's overflow
trap does not. Emit an aborting trap helper. For the initial foreign-call slice,
require an explicitly agreed
[`panic=abort` build graph](https://doc.rust-lang.org/rustc/codegen-options/index.html#panic)
or reject integration; never silently change a Rust host's panic policy.
Supporting an unwinding host later needs a separate boundary contract, including
callbacks and destructor panics. Wrapping a whole generated function in
[`catch_unwind`](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
is insufficient: cleanup may already have run before the catch, and aborting
panics are not caught.

Plenty also has [required direct recursive tail transfers](../../design/language/06-functions/05-tail-calls.md).
An ordinary generated Rust call cannot be assumed to preserve that guarantee.
Translate admitted self-tail recursion to a loop, use an explicit dispatch
machine for supported mutual cycles, or reject the subset until implemented.
Generic specialization must precede that analysis where it does today.
Generators need explicit frame, suspension and cleanup lowering; a Rust
iterator or async future is not automatically the same lifecycle. Concurrency,
generators, boxed closures and runtime-specific intrinsics are outside a first
library slice.

## Generic and tooling boundaries

The present frontend specializes generics before native lowering. Emitting
those instances as private Rust functions is simpler than recovering generic
templates, but only exposes selected monomorphic APIs to Rust. Exporting a
generic Plenty function as a Rust generic requires retaining the template and
translating protocol bounds, not mechanically replacing bracket syntax.
Rust trait coherence/orphan rules still apply to generated implementations;
newtype facades may be necessary for foreign traits/types.
[Rust implementation rules](https://doc.rust-lang.org/reference/items/implementations.html#trait-implementation-coherence).

Keep selected imports explicit first. Cargo metadata describes packages, not
complete type/loan semantics. Automatic API discovery, procedural macros,
associated types and async executors remain independent integration problems.
The compiler should track a generated-byte-range to Plenty-span map, consume
rustc JSON diagnostics, and distinguish user API mismatch from emitter failure.
Path remapping alone cannot map generated Rust expressions back to Plenty.
Runtime trap sites and debugger stepping are separate obligations. Neither
source mapping nor debugger support has been implemented by this exploration.
