# Using Rust crates in Plenty

> **Status: Not implemented; comparative prototype research (2026-10-23).**
> B17 owns dependency tooling; backend alternatives relate to B20. Current
> [packages](../design/toolchain/01-building/packages.md),
> [linking](../design/toolchain/01-building/linking.md) and
> [runtime packaging](../design/toolchain/02-architecture/runtime.md) are unchanged.
> The prototypes are experiments, not a new supported compiler mode.

## Recommendation

The most tractable way to let Plenty projects use Rust crates is **generated
C-ABI adapters over a Cargo-built Rust dependency graph**, with Rust-owned opaque
objects and checked Plenty-facing bindings. Users select Rust APIs, not C APIs;
upstream crate authors need neither rewrite their implementation nor provide C
exports. Cargo and rustc still consume ordinary `rlib` build artifacts.

This broadens the original investigation beyond avoiding C ABI. Executed
prototypes show that Rust ABI does not eliminate the ownership/marshalling layer
for Plenty's different value representations, and it adds compiler-version and
lowering obligations. A Rust-source backend removes that boundary for supported
types, but preserving the whole Plenty language/runtime is a much larger job.
The recommendation concerns implementation cost and maintenance, not a claimed
universal performance winner.

Keep native Rust ABI as an opt-in research route. Keep a Rust-source backend as
a separate backend project if access to Rust-native types/traits/async becomes
a central goal. [Evidence and measurements](rust-crate-dependencies/evidence.md)
describe all three; the [project/value contract](rust-crate-dependencies/contract.md)
sets the intended user-facing boundary.

## What the three prototypes establish

All paths execute one source workload containing scalar calls, Unicode/NUL text,
a generic identity operation, a Rust `regex` owner with query and destruction,
and a local Rust Future polled from Pending to Ready. Source is checked by the
existing Plenty frontend against generated binding signatures, then a small
restricted parser lowers its actual operations. Both native routes emit a real
Cranelift object. The third emits actual Rust source, not a substituted result.

| Route | Demonstrated | Remaining production work |
| --- | --- | --- |
| Rust-ABI adapters | Real Cranelift calls to pinned scalar/pointer Rust-ABI functions, debug/release adapter builds; separate rustc-private ABI queries work. | Connect matched compiler ABI information to lowering, pin compiler/helper identities, validate every admitted type and upgrade. |
| Generated C adapters | Same selected Rust APIs and ownership lifecycle, generated adapters and caller declarations, stable Rust compiler interfaces. | Integrate generated modules/contracts with Plenty project tooling, error/allocation policy and runtime linking. |
| Plenty-to-Rust source | Checked restricted Plenty calls and arithmetic become Rust calls, native Rust owners and borrows; no opaque-handle adapter boxes. | Backend for checked IR, complete Plenty representations/semantics, std/runtime parity, diagnostics and compile-time qualification. |

A separate pinned nightly helper queried `FnAbi`: `i64` was direct, `&str` a
pair, and a 24-byte `repr(C)` record passed indirectly by pointer under Rust ABI
but on the stack under C ABI. **`repr(C)` fixes layout, not the function calling
convention.** This makes compiler-assisted Rust ABI handling plausible, while
showing why the successful scalar calls must not be generalized. The helper
observes a different compiler build from the stable executable tests and is not
connected to their Cranelift lowering.

## rlib consumption and final linking

An rlib contains compiler-specific metadata and code, potentially LLVM bitcode.
It is not an API schema or a bundle of all upstream Rust dependencies. Generic
instances may be generated downstream; rustc must participate. Metadata from
another compiler version is rejected.
[Libraries and metadata](https://rustc-dev-guide.rust-lang.org/backend/libs-and-metadata.html),
[monomorphization](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html).

Rust's supported mixed-language linking routes are a rustc-owned final link or
one aggregate Rust `staticlib` linked by a foreign driver. Passing arbitrary
rlibs directly to a foreign linker remains unsupported. Artifact format and
calling convention are independent: **C-ABI functions can live in rlibs**.
Both adapter prototypes use rustc to link their rlibs and the Cranelift object.
[Linkage](https://doc.rust-lang.org/reference/linkage.html#mixed-rust-and-foreign-codebases).

For production, prefer a Cargo-managed final Rust target that includes the
generated adapters, dependency graph, rebuilt Plenty runtime and application
object. Alternatively produce one aggregate staticlib for today's linker.
Neither route means converting upstream crates to C libraries individually.

## Recommended boundary

```text
Plenty manifest + selected Rust APIs / small Rust companion module
                     |
                generated bindings
               /                  \
Plenty checked foreign owners     Rust C-ABI adapters
          |                       ordinary Rust calls
    Cranelift object              Cargo dependency rlibs
               \                  /
             one runtime graph, rustc final link
```

Generate exact-width scalar/pointer/length interfaces and explicit output/error
contracts. Keep complex Rust values opaque; Rust constructs, borrows, mutates
and drops them. Instantiate selected generic operations inside Rust. Plenty
enforces owner moves, exclusive/shared loans and drop timing across calls.
Binding metadata must specify these rules, not merely the machine signature.
Automatic discovery can assist API selection, but Cargo metadata and symbol
lists are insufficient to derive every lifetime/effect contract safely.

A companion `.rs` module is a practical escape hatch for trait-heavy APIs,
callbacks and async executors. It can present a small reusable operation to
Plenty while calling an unchanged crate. Publish/cache adapters independently
of individual applications where their API and crate-version contract allows;
do not make every user write unsafe pointer glue. Unsupported APIs should give
a precise diagnostic and suggest a facade, not silently erase types/errors.

The prototypes' async operation is a future with explicit polling, not async
Plenty syntax or a reactor integration. C ABI does not prevent async use: an
opaque operation can expose start/poll/cancel with owned state. Real executors,
wakers, callbacks, retained buffers and cancellation completion need explicit
lifecycle contracts before general async crate support.

## Runtime and compiler integration

Today `src/codegen.rs::link_object` extracts an embedded staticlib and
`src/toolchain.rs::link` invokes a cc driver. `build.rs::build_runtime` builds
Rust std, vendored dependencies and an abort-on-panic runtime into that archive.
Appending another independent Rust staticlib risks conflicting allocator,
panic/std and native library implementations.

For Rust-enabled projects, ship the matching Plenty runtime source/build recipe
and rebuild it in the same Rust graph. Reuse the runtime variant without C main
and a Rust launcher, preserving Plenty application initialization and exit
status; or produce one aggregate staticlib with the existing startup contract.
Keep today's embedded-runtime mode for pure Plenty projects. None of the three
experiments integrates the real Plenty runtime into its final executable.

Project resolution and caching belong in `src/project.rs`; imports receive a
resolved graph rather than fetching during type checking. Add foreign-owner
types and loan/drop operations before backend lowering. Map generated Rust
diagnostics back to binding requests. Stable C ABI removes dependence on
compiler-private lowering, **not** the need for compatible Rust artifacts
within each Cargo build.

The Rust-ABI alternative would obtain a monomorphic adapter's `FnAbi` and layout
from a matched helper and translate every accepted pass mode and attribute.
Reject unsupported cases. Compiler APIs are unstable; rustup's `rustc-dev` and
`llvm-tools` enable investigation but do not make a portable ABI contract.
[ABI rules](https://doc.rust-lang.org/reference/items/external-blocks.html#abi),
[FnAbi](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/struct.FnAbi.html),
[external drivers](https://rustc-dev-guide.rust-lang.org/rustc-driver/external-rustc-drivers.html).

## Other boundaries and next evidence

Wasm components could supply isolation and a defined value-transfer interface,
but need a Wasm runtime, supported crate targets, bindings and copy/lifetime
policies. A subprocess/RPC boundary offers strong isolation and independent
toolchains at the cost of serialization, process lifecycle and latency. These
are useful for coarse operations, not substitutes for cheap in-process calls.
Neither was prototyped here.

The next adoption gate is one real Plenty executable using a generated adapter,
recoverable constructor errors, deterministic cleanup and a unified runtime
graph. Then qualify a native build-script dependency, realistic async facade,
offline rebuilds and version updates. Source-backend expansion and full native
Rust ABI lowering remain separate decisions. The current evidence supports
generated C adapters as the smallest route to useful crate reuse; it does not
justify promising that every crate API will be directly importable.
