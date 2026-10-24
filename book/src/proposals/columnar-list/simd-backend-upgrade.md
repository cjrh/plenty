# SIMD after the Cranelift 0.137 upgrade

The automatic column filter/count slice was rebuilt and revalidated against
Cranelift **0.137.0**, after merging local main through `c6e3f8b9`. The original PR used
0.131.1. Production lowering needed only the new `MemFlagsData` type and the
explicit signed-immediate builder methods (`band_imm_s`, `iadd_imm_s`,
`ishl_imm_s`, `icmp_imm_s`). The loop's legality checks, four-lane width, baseline
target and scalar tail are unchanged.

## Fresh evidence

Post-integration validation passed 145 compiler tests, 3,787 integration tests
(two ignored), 183 runtime tests (four ignored) and five example tests. The
full workspace/all-features run exposed issue #442 in the newly merged
trap-table test: heap-to-stack offsets can exceed its signed 32-bit range.
Keeping its synthetic code/sites/table in one allocation fixes the test without
changing production behavior; the entire runtime suite then passed in both
default and all-features configurations. This includes signed/unsigned comparison and fallback tests,
generated-code checks and protected-page scans at lengths 0–129. Clippy passed
for all workspace targets with warnings denied. Both compiler profiles were
rebuilt from an empty task target directory; compiler debug information was
disabled to keep build storage bounded.

The final [four-placement measurements](simd-counts.md#final-measurements)
use the new release compiler. Its SIMD loop keeps the comparison constant in
a register instead of reloading it each four-row iteration. The measured large
scans execute about 0.25 fewer instructions per row. At the initial backend
upgrade, scalar control assembly was unchanged while SIMD register allocation
and block layout changed. Subsequent main changes replace executable trap
helpers with PC-relative trap tables, shrinking the SIMD/scalar functions from
608/525 to 592/513 bytes. Measurements were repeated after integration; retired
instructions per scanned row remain the same. Direct reference-store changes
introduce no new alias-region assumptions. We do not
attribute all elapsed-time differences to one optimizer pass.

## Vector-width probe

`examples/clist_simd_backend_probe.rs` compiles a load/compare/store function
with the verifier enabled and the workspace's pinned dependencies. The
experiment instructions (`experiments/simd/clist/README.md` in the repository)
reproduce it. On the Ryzen 7 7840HS:

| IR type | Baseline x86-64 | Detected native features |
| --- | --- | --- |
| `i32x4` (128 bits) | Compiles, SSE encodings | Compiles, VEX encodings using XMM registers |
| `i32x8` (256 bits) | Rejected | Rejected |
| `i32x16` (512 bits) | Rejected | Rejected |

Both rejected widths report `Unexpected SSA-value type`. This is a compile-only
probe, independent of Plenty's pattern recognition. The pinned upstream
[x86 register-class selection](https://github.com/bytecodealliance/wasmtime/blob/v50.0.0/cranelift/codegen/src/isa/x64/inst/mod.rs#L1419)
accepts vector SSA values up to 128 bits. Native feature detection changes
available instructions but does not remove that restriction. The versioned
upstream file was checked against the installed crate source.

## Useful upstream changes

Cranelift 0.137 corresponds to Wasmtime v50. Release notes across v45–v50
describe optimizer and target improvements. In particular,
[v48](https://github.com/bytecodealliance/wasmtime/releases/tag/v48.0.0)
adds dead-store elimination in alias analysis and VNNI dot-product lowering;
[v50](https://github.com/bytecodealliance/wasmtime/releases/tag/v50.0.0)
improves alias analysis and introduces initial APX support. APX is not a wider
SIMD-vector facility. Winch SIMD entries concern a different compiler backend.

The pinned x86 source has new AVX-VNNI and AVX-512-VNNI feature flags and
[dot-product patterns](https://github.com/bytecodealliance/wasmtime/blob/v50.0.0/cranelift/codegen/src/isa/x64/lower.isle#L4088)
for packed signed/unsigned byte multiplication and accumulation. These could
help a future quantized or packed-byte column calculation, provided its
semantics and target selection are established. They do not replace this
integer comparison/count kernel. No VNNI lowering, runtime dispatch or public
SIMD API is added here; broader priorities remain in the [backlog](../../backlog.md).

The upgrade improves the backend we already use without requiring a broader
ISA policy for this slice. It does not demonstrate general auto-vectorization
or unlock ordinary 256/512-bit vector SSA operations on x86.
