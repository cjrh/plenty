# Rust crates as project dependencies

> **Status: Not implemented; research proposal (2026-10-23).** Proposed under
> B17, with backend work related to B20. [Packages](../design/toolchain/01-building/packages.md),
> [linking](../design/toolchain/01-building/linking.md), and
> [runtime packaging](../design/toolchain/02-architecture/runtime.md) describe
> current behavior. No Rust imports, manifest keys or ABI support ship here.

## Recommendation

Reusing Rust crates is tractable if **Cargo and rustc participate in the Plenty
build**. Treat `rlib` files as cached Rust compiler inputs, not a portable foreign
library format. Investigate generated Rust-ABI adapters with a toolchain-matched
compiler helper before committing to implementation. Upstream Rust source stays
unchanged; crate authors need not publish C interfaces.

Adding a dependency and exposing every Rust API are different goals. The first
can support a useful checked subset; the second would entail much of Rust's type
system. Initially, selected APIs need a binding specification and sometimes a
small Rust facade. This is still much less work than translating a crate.

See the [project/value contract](rust-crate-dependencies/contract.md) and
[research evidence](rust-crate-dependencies/evidence.md). The recommendation is
an architectural inference, not a demonstrated Plenty integration.

## What an rlib does and does not provide

Rust explicitly treats passing rlibs directly to foreign linkers as unsupported.
Supported mixed-language linking uses rustc as final driver or a single aggregate
Rust `staticlib`. An rlib does not bundle the Rust dependency closure. These are
packaging rules, not requirements to use C ABI.
[Rust linkage](https://doc.rust-lang.org/reference/linkage.html#mixed-rust-and-foreign-codebases).

The archive contains compiler-specific metadata and code, potentially LLVM
bitcode rather than native instructions. Metadata identifies the producing
compiler; rustc rejects other compiler versions. A symbol table is not an API
schema. [Libraries and metadata](https://rustc-dev-guide.rust-lang.org/backend/libs-and-metadata.html).

Generic functions may have no callable machine-code instance until a downstream
Rust crate asks for concrete types. Rust performs monomorphization; a linker
cannot type-check or instantiate MIR.
[Monomorphization](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html).

Rust ABI has no stability guarantee. Default Rust record layout does not promise
source field order. Matching field names or sizes cannot justify reinterpreting
Plenty strings, sums, records or ownership descriptors as Rust values.
[ABIs](https://doc.rust-lang.org/reference/items/external-blocks.html#abi),
[layout](https://doc.rust-lang.org/reference/type-layout.html#the-rust-representation).

## Architecture to investigate

```text
Plenty manifest + selected Rust API bindings
        |
Cargo-resolved graph + generated Rust adapter crate
        |                               |
rustc checks/instantiates calls     matched helper emits ABI/binding contract
        |                               |
dependency/adapter rlibs            Plenty checks loans, emits Cranelift object
        |                               |
        +------ rebuilt runtime rlib ---+
                         |
                  rustc owns final link
```

Adapters are ordinary Rust functions using Rust ABI. Their Rust bodies call
upstream APIs, leaving trait resolution, associated types, generic instances and
upstream layout to rustc. Stable generated symbol names identify adapters rather
than guessing upstream mangled names. A stable symbol name does not fix its ABI.

Keep adapter interfaces small: fixed-width scalars, thin pointers, lengths and
output slots. Rust constructs native references/values inside the adapter after
the generated contract establishes validity. Complex values remain behind owned
handles. This is explicit marshalling over **Rust ABI**, not a promise of shared
Rust/Plenty representations or universal zero-copy calls.

The missing component is a pinned rustc helper that checks selected APIs and
exports the exact adapter ABI plus semantic contract. Cranelift must implement
argument/return lowering, extensions, alignment and hidden arguments. Merely
setting `SystemV` is insufficient. Internal `FnAbi` and `PassMode` distinguish
direct, pair, indirect, ignored and cast arguments; they are implementation
starting points, not stable protocols.
[FnAbi](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/struct.FnAbi.html),
[PassMode](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/callconv/enum.PassMode.html).

Reject unsupported lowering. Initially validate only constrained adapter shapes
on `x86_64-unknown-linux-gnu`. If ABI extraction is too fragile, evaluate a rustc
codegen integration that emits boundary calls itself. Generating a Rust wrapper
alone does not solve how the Cranelift caller reaches it.

Compiler internals require a matched helper and pinned supported toolchain,
potentially nightly with `rustc-dev` and `llvm-tools`. Any installed stable
rustc is not sufficient for this design. Assuming rustup makes selecting the
required toolchain reasonable, but Plenty must publish its exact identity and
diagnose mismatches. [External compiler drivers](https://rustc-dev-guide.rust-lang.org/rustc-driver/external-rustc-drivers.html).

## Integration with Plenty

At the researched revision, `src/codegen.rs::link_object` extracts the embedded
runtime and `src/toolchain.rs::link` invokes a cc-compatible driver.
`build.rs::build_runtime` builds an optimized ThinLTO, abort-on-panic `staticlib`.
It includes Rust std and vendored dependencies and supplies C `main` for
applications. Ordinary Plenty functions use private conventions, including
Cranelift tail calls. Existing C imports do not implement Rust ABI lowering.

For Rust-enabled projects, rebuild the **matching Plenty release's runtime
sources** in the same Cargo/rustc graph as the adapters and user crates. Ship
sources, vendored inputs and the build recipe as an optional toolchain component.
Do not append a new arbitrary Rust graph to today's embedded staticlib: that
risks conflicting std, allocator, panic and native-library implementations.
Keep the embedded runtime for projects without Rust dependencies.

Use a runtime library variant without C `main` and a generated Rust launcher
calling Plenty through its established entry contract. Factor initialization
so application signal/stack handling and exit status remain equivalent. Existing
internal C runtime calls may remain; the new crate boundary is still Rust ABI.
Require one coherent std/allocator/panic policy and diagnose native conflicts.

`src/project.rs` should own resolution, generated workspaces and fingerprints.
The compiler receives a resolved binding graph, rather than fetching during
imports. Add foreign-owner loan/drop operations before codegen. Preserve rustc
diagnostics and map generated errors back to binding requests. Checking and LSP
may require Rust builds for macros/bindings even without a final executable.

## Alternatives and adoption gates

| Route | Assessment |
| --- | --- |
| Pass rlibs to today's linker | Useful probe; no general metadata processing, generics or ABI support. |
| Decode metadata and reimplement Rust semantics | Possible with a pinned compiler, but a large compatibility/type-system burden; avoid independent decoding. |
| Rust-ABI adapters plus matched helper | Recommended investigation; constrained boundary still needs compiler integration. |
| rustc codegen plugin/backend | Stronger ownership of boundary lowering, more unstable compiler coupling. |
| Emit a Rust backend for Plenty | rustc owns calls throughout, but this is a separate substantial backend project. |
| C adapters plus aggregate staticlib | Simpler stable-Rust fallback; does not meet the non-C-ABI goal and needs an explicit design decision. |

First prove actual Cranelift calls to generated Rust-ABI adapters across debug
and release optimization, including mismatch rejection. Then build a complete
Plenty executable with a rebuilt runtime and one generic allocating crate.
Finally qualify a parser/regex-style API and a native build-script dependency,
including errors, cleanup and offline rebuilds. These are technical dependencies,
not a second delivery queue. Keep cross-language LTO off initially.

Measure clean/incremental build time, binary size, call overhead, copies,
allocations and toolchain-upgrade maintenance. A scalar archive link is not an
adoption gate. If a safe bounded interface requires too much rustc machinery or
cannot preserve Plenty ownership, reconsider the C-adapter fallback explicitly.
This is a substantial compiler/tooling feature, not an extra linker flag.
