# Proposed Rust dependency contract

Companion to [Rust crate dependencies](../rust-crate-dependencies.md).
All syntax and behavior below are proposed, not implemented.

## Project declaration

Distinguish Cargo crates from Plenty packages. One possible spelling is:

```text
[rust-dependencies]
matcher = { package = "regex", version = "1", default-features = false, features = ["std"] }

[rust]
toolchain = "<project-selected supported Rust toolchain>"
bindings = "rust-bindings.toml"
```

This names a candidate, not a qualified crate. Registry versions, local paths
and pinned Git sources should reuse Cargo semantics. Generate a deterministic
private workspace with explicit manifest, target, profile and toolchain.
Cargo owns feature resolution and transitive dependencies, including host
proc-macros/build scripts. Use versioned metadata and build JSON for actual
artifact paths, not guessed hash-suffixed filenames.
[Cargo external tools](https://doc.rust-lang.org/cargo/reference/external-tools.html).

The recommended generated C-adapter route uses stable Rust interfaces. Pin the
project toolchain for reproducibility; an exact compiler-private helper match
is required only for the experimental Rust-ABI route. A Rust companion module
can simplify unsupported APIs while the generator supplies the ABI glue.

Commit a Cargo-format companion lockfile, for example `plenty.rust.lock`, and
record its digest and toolchain identity in the Plenty lock/build contract.
`--locked` forbids changing either lock. `--offline` applies to both ecosystems
and fails on missing sources/components. Imports never silently install a
toolchain. Cargo builds execute dependency build scripts and proc-macros; this
is not a data-only import.

Cache keys include compiler/runtime identity, adapter/schema versions, full
rustc identity, target/CPU, Cargo lock and resolved features, profile/panic/LTO
flags, relevant environment/configuration and source/build inputs. Honor Cargo
invalidation; a lockfile is not a fingerprint of mutable path sources. Rebuild
after toolchain changes. Arbitrary prebuilt rlibs without a verifiable compatible
graph are unsupported.

Leave native flags, search paths and build-script ordering with Cargo through
the final Rust target. Its `links` constraint prevents competing packages from
claiming the same native library. Account for the runtime's vendored native
dependencies in that graph, rather than bypassing the checks.
[Build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html).

## API selection and value rules

A binding specification selects public Rust paths, concrete type arguments and
Plenty-facing signatures. rustc checks generated Rust calls, including traits,
visibility and enabled features. Cargo metadata and symbol lists alone cannot
describe callable APIs. Safe Rust signatures do not establish the validity of
raw-pointer adapters: the generated boundary must enforce its own preconditions.

A first useful surface could expose a constructor, borrowed query and destructor
for an opaque Rust matcher/parser, imported as a generated Plenty module under
the dependency alias. A small Rust facade can simplify unsupported APIs without
translating the crate. Automatic discovery can later prefill binding selections.

| Rust shape | Initial proposed treatment |
| --- | --- |
| Integers/floats/bool | Exact-width mapping, checked conversions and bool validity; target-check platform-sized integers. |
| `&str`, `&[u8]` inputs | Scoped validated pointer/length views when storage permits, otherwise explicit copies; no retention after return. |
| Owned records, `String`, `Vec<T>` | Opaque move-only owners, constructed/dropped in Rust; explicit copy-out where needed. |
| `Option<T>`, `Result<T,E>` | Destructure in Rust into specified tags and initialized outputs; preserve typed/opaque errors, never reinterpret enum layout. |
| Generic calls | Selected concrete Rust instantiations; no automatic Rust trait implementation for arbitrary Plenty classes. |
| Shared/mutable methods | Shared/exclusive loans on the owner; forbid aliases and use after consuming calls. |
| Borrowed returns | Initially reject or explicitly copy; general Rust lifetime relationships exceed current Plenty returned-borrow rules. |
| Closures, trait objects, async, retained callbacks | No automatic general import; allow a reviewed Rust facade with a defined environment/lifetime/executor contract. Explicit future polling works in the prototype, but no production executor contract exists. Macros run in Rust, not as Plenty syntax. |
| Unsafe APIs/raw pointers | Separately audited safe facade, not automatic safe exposure. |

Boxing an opaque owner is an explicit fallible bridge allocation. Rust crates
may allocate internally and abort on OOM: they do not automatically gain
Plenty's recoverable `AllocError` guarantee. Document copies and allocation
effects. Deallocation stays with the originating runtime even if both use the
system allocator. Drop exactly once on normal/error paths; invalidate consumed
handles and specify output initialization separately for success and failure.

Rust values are not automatically Plenty-copyable or worker-safe. Start with
thread-bound owners; later worker exposure needs compiler-checked `Send`/`Sync`
and compatible Plenty rules. Pinned/self-referential objects need an explicit
storage contract. Rust checks the adapter's local borrows; Plenty must check
lifetimes and aliasing across successive calls.

## Failure and qualification

Use abort-on-panic initially, matching today's runtime. Panic terminates the
process rather than becoming `Result`; `catch_unwind` cannot catch an abort.
No Rust unwind may escape through Plenty frames. A later catch-at-adapter policy
needs compatible unwind builds and proof for every call and destructor.

Qualification needs compile-fail tests for unsupported APIs, aliasing and
toolchain mismatches; drop counters across moves/errors; output-initialization
checks; embedded-NUL/non-ASCII text; narrow integers and return lowering; panic
in a subprocess; generics; a native dependency; and Rust owners alongside Plenty
allocations. Check duplicate runtime/native symbols and cold/offline builds.
The [prototype evidence](evidence.md) covers a subset: real Cranelift callers,
generated adapters, a restricted Rust-source path, move rejection, cleanup,
UTF-8/NUL input, generic calls, Pending/Ready polling and arithmetic traps.
Production project/runtime integration and the broader matrix remain unimplemented.
