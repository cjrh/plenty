# Cosmo compile-time target and code-generation contract

> Proposed, not implemented. Part of the [Cosmopolitan proposal](../cosmopolitan.md).
> These contracts precede [runtime qualification](evidence-and-gates.md), not
> merely final packaging.

## Build target is not the runtime operating system

Today `src/frontend/target.rs` derives facts from the packaged native triple,
recognizes a closed set of `target.os` values, and discards unselected branches
while parsing, before resolution and checking. See the implemented
[target conditionals](../../design/language/02-source/target-conditionals.md).
Using that native fact source for an APE would silently select Linux-specific
code on a Linux compiler host, even when the artifact later runs elsewhere.

Recommend adding the closed value **`"cosmo"`** for `target.os` and deriving all
facts from the explicit output-target descriptor, never the compiler host.
Here `os` denotes the compile-time platform contract, not the executing kernel.
For the proposed x86-64 profile the facts are `os = "cosmo"`, `arch = "x86_64"`,
`pointer_width = 64`, and `endian = "little"`. A later qualified AArch64 slice
would have `arch = "aarch64"`; fat output evaluates conditionals independently
for each slice. This does not introduce runtime dispatch or a runtime OS API.
Runtime detection facilities are outside this proposal.

For Cosmo, existing comparisons with `"linux"`, `"macos"`, `"windows"` and
`"freebsd"` remain valid but equality is false and inequality true. A chain
selects its first true arm, or its `else`, just as native conditionals do.
Thus an existing Linux/else program selects its else arm: that arm must be
portable or explicitly reject Cosmo through capability checking. An else arm
is not evidence of portability. Unknown spellings remain errors. Native targets
retain their existing values and branch behavior; recognizing `"cosmo"` adds a
value to the closed set, not an alias for any native OS. Older compilers will
reject that new value when evaluated, so using it requires the new compiler.
Unselected arms remain syntactically parsed but are not resolved/type-checked;
this proposal does not change that language rule.

Required tests cover top-level selected/unselected imports (including absent
modules), declarations and function-body branches, first-match/else and no-match
chains, all native OS equality/inequality comparisons, boolean combinations,
unknown-value diagnostics and native regressions. Compare identical Cosmo
selection across compiler hosts wherever those hosts are supported; unit tests
must independently vary host and output descriptors even during Linux-only
experimentation. Cache tests must isolate native versus Cosmo and per-ISA/profile
entries, including a source whose selected import changes. Include the output
facts/conditional-semantics version in the shared target/cache identity so parsed
or compiled results cannot be reused under a different platform contract.

Reject host-OS aliasing: it defeats multi-OS output. Reject treating `target.os`
as a runtime expression: discarded imports cannot be restored at launch.
Reject initially banning all OS conditions in favor of the explicit Cosmo value:
a dedicated arm gives portable libraries a direct migration path while keeping
native-only arms unselected. Capability rejection still guards unsupported calls
in the selected code; OS comparison alone is not a capability check.

## Early x86-64 red-zone and code-generation gate

The retained 4.0.2 `x86_64-unknown-cosmo-cc` wrapper (around lines 119–128)
passes `-mno-red-zone`, except in its Linux-only `optlinux` mode, which selects
`-mred-zone`. Multi-OS output must not assume `optlinux` compatibility. In contrast,
`build.rs` invokes rustc with Cargo's GNU target and no explicit no-redzone
option. This is a compatibility risk requiring inspection, **not evidence that
red-zone use caused the observed trap failure**. The signal-layout mismatch is
measured; its causal role in that failure remains a hypothesis too.

Before any runtime-compatibility acceptance, audit Plenty-generated objects,
Rust runtime code, bundled Rust std/compiler-builtins, vendored Rust crates and
native dependency/assembly objects. Retain effective backend settings, rustc
arguments/target specifications, C/assembly build commands, archive-member and
reachable-symbol inventories, and disassembly covering leaf functions, spills,
stack probes and signal/worker paths. Establish whether reachable functions use
storage below the current stack pointer without reserving it, accounting for
control flow and inlining/LTO; a textual search for negative offsets alone is
not a proof. Unreachable members need a recorded reachability argument and
profile enforcement, not an assumption that all dependencies disappear.

The recommended multi-OS contract is no red-zone reliance in reachable code.
Verify the pinned Cranelift configuration and emitted code; configure/rebuild
Plenty objects if necessary. Use `-C no-redzone=yes` for rebuilt Rust components
and compatible C/assembly settings where required. A final Cosmo linker flag
cannot retroactively recompile emitted Plenty objects, Rust std or archives.
If an incompatible reachable member is found, rebuild its owning component
(including std/compiler-builtins if implicated), replace it, or reject the
capability/profile; then repeat the audit and behavioral tests. Do not assert
that every Rust dependency must be rebuilt without that evidence. If provenance
or analysis cannot establish compatibility, the gate stays closed. Record the
verified code-generation policy and affected input hashes in archive/cache
identity so changing the policy invalidates old objects.
