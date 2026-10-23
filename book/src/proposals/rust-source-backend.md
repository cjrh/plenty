# Compiling Plenty to Rust source

> **Status: Exploratory proposal, not implemented (2026-10-24).**
> B20 tracks optional backends; B17 tracks Rust dependency tooling. The current
> compiler still emits native code through Cranelift. No new command, target,
> Rust import syntax or compatibility guarantee is introduced here.

## Recommendation

**This merits a bounded backend experiment, especially for Rust-hosted Plenty
libraries.** Emitting a Rust crate could let Rust call Plenty functions, and
Plenty call selected Rust APIs, with ordinary Rust owners, borrows, enums and
callbacks. rustc would own type layout, calling conventions, generic codegen
and final linking within that build. Users could write application logic in
Plenty while retaining a Rust application's ecosystem and tooling.

The opportunity is real; “seamless” needs a precise boundary. Producing valid
Rust is easier than preserving Plenty's semantics **and** exposing a natural
Rust API. Replacing runtime values with `Vec`, `String` and derived `Drop`
everywhere would change allocation, copying and cleanup behavior. Keeping all
current runtime representations would preserve more behavior but expose a
Plenty runtime API to Rust users. Prefer a small shared native-type subset,
with explicit wrappers for richer Plenty values.

The [Rust crate investigation](rust-crate-dependencies.md) already includes an
actual restricted Plenty-to-Rust emitter alongside two native adapter routes.
This proposal develops the source-backend direction as its own architectural
choice. It does not overturn that investigation's lower-cost recommendation
of generated C adapters for adding Rust dependencies to today's compiler.

## Proposed compilation boundary

```text
Plenty source ── parse / resolve / type and ownership checks
                                 |
                 typed source structure + places, loans,
                  cleanup decisions and source locations
                                 |
                       generated Rust crate
                       /                 \
             Plenty support crate     selected Rust APIs
                       \                 /
                         Cargo + rustc
                              |
                  Rust host library or executable
```

Keep Plenty's own checker authoritative. Do not translate unchecked source and
use Rust errors as Plenty's type system: their admitted borrows and contracts
differ. The independent Rust check is valuable defense and catches emitter
mistakes, but a supported Plenty program rejected by emitted Rust is a backend
problem, not a reason to silently redefine the language.

Start from the [existing structured operations](../design/toolchain/02-architecture/frontend.md)
and their construction sites, not Cranelift IR or assembly. `src/op.rs` already
retains typed slots, structured branches/loops, `Loan`, `Access`, `Site`, `Try`
cleanup and `DropLocal` operations. It is not devoid of ownership information.
However, some source constructs are specialized/desugared and runtime operations
already assume Plenty layouts. No public checked-IR emission API exists today.

The next technical question is whether these facts suffice to reconstruct a
small safe Rust body, or whether a thin typed source representation must be
retained before representation-specific lowering. Prefer preserving the missing
facts at that boundary to reverse-engineering lifetimes from machine slots.
Do not commit to a new universal IR before the small slice answers that question.

## User-facing direction

The first useful product would be a Rust-consumable library containing selected
Plenty functions. A generated crate would have stable exported Rust names and
an explicit API manifest; internal monomorphized names remain private. Rust
can supply native data, call these functions and inspect native `Result`s.
Selected Rust imports would use checked signatures or a small Rust companion
module whose ownership/effects are declared. A raw symbol list is insufficient.

Build integration can use an explicit Plenty generation step followed by
Cargo, or a build script writing generated modules to `OUT_DIR` with complete
input invalidation. Cargo already supports this kind of
[source generation](https://doc.rust-lang.org/cargo/reference/build-scripts.html).
Separate generation from dependency resolution so checking does not unexpectedly
fetch or execute build scripts. Pin the supported Rust toolchain/edition and
lock dependencies; build one coherent runtime/dependency graph.

This is source interoperability inside a coordinated Rust build, not a stable
Rust binary ABI or a promise to mix arbitrary `rlib`s. The
[Rust ABI has no stability guarantee](https://doc.rust-lang.org/reference/items/external-blocks.html#abi).
Native Cranelift Plenty objects cannot directly treat emitted Rust layouts as
their existing layouts. Mixed-backend calls still need a defined boundary.

## Why pursue it, and what it does not buy automatically

| Potential benefit | Qualification |
| --- | --- |
| Rust calls Plenty as a normal crate | Public types and lifetime contracts must be designed, not just printed. |
| Plenty calls Rust without C marshalling | Only admitted types/APIs share that representation; ownership and effects still need checking. |
| LLVM optimization and Rust targets through rustc | No speedup or new Plenty target is established until runtime support, ABI boundaries and generated code are tested. |
| Cargo packaging, profiling and debugging | Generated-source locations need mapping back to Plenty; dependency build cost remains. |
| Access to trait-heavy or async libraries | A companion Rust facade can help now; arbitrary traits, macros and futures do not become Plenty features merely by changing backends. |

The fast Cranelift path remains valuable. A Rust-source route is a stronger
candidate for ecosystem integration or an optional optimizing build than a
demonstrated replacement for interactive compilation. Two frontends plus Rust
type checking and optimization add work; incremental Cargo builds may amortize
some of it. Measure cold, unchanged and edited builds separately, including
generation, dependency build, Rust compilation, link time and peak memory.

The [semantic contract](rust-source-backend/contract.md) describes the hard
boundaries. [Probe evidence and adoption criteria](rust-source-backend/evidence.md)
explain what has run and what would justify a first implementation. Future work
and priority remain in the [backlog](../backlog.md), not this proposal.
