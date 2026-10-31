# Completed compiler performance and SIMD work

[Completed-work index](../completed-work.md) · [Remaining work](../backlog.md)

These are delivery/investigation records, not current benchmark guarantees or
an optimization queue. Workloads, methods and current measurements belong to
[profiling](../development/profiling.md); remaining B19/B23/B74 scope belongs
only to the [backlog](../backlog.md).

## B19: Generated code and storage

- The CPU audit (`examples/cpu_codegen_audit/`, local PR #431) led to issues
  #360–#392. `--dump` exposes operations/CLIF/assembly (issue #360). Unborrowed
  locals stay in registers, narrow helpers replace dispatcher calls in borrowed
  loops/byte/text operations, last owned reads move, held text avoids retain/
  release, and SSE2 rounding is inline (issue #386).
- Separate `plenty_op_<N>` symbols permit native section GC. The measured
  stripped hello executable fell from 790 KB to 356 KB (issue #501,
  local PR #577). Default stripping and lean runtime linkage are also recorded
  under [B17](language-and-tooling.md#b17-tooling-and-packaging).
- Dense integer/tag matches use bounded jump tables at 256+ distinct values
  (issue #395; [measurements](../development/profiling.md#dense-match-dispatch)).
  String matches with 16+ literals binary-search one sorted table
  (issue #565; [measurements](../development/profiling.md#string-match-dispatch));
  other shapes retain chains.
- Inline class field copies/address rewrites and destructor-free release for
  up to eight owned fields shipped (issue #544, local PR #663). One-field
  destructor-free classes use their field representation (issue #547,
  local PR #666); class fields are flat and strings release in place
  (local PR #668).
- Inline copies/clears up to 192 bytes, in-place moved arguments and descriptor
  string-offset cleanup plans shipped (issue #550, local PR #669).
  Class components of tuples/multifield variants are flat; single-field
  Option/Result payloads keep the address layout (issue #551, local PR #670).
  See [flat-layout evidence](../proposals/flat-class-layout.md).
- Release builds inline small constructors/`__init__`/`new`, avoid release of
  fields initialized first and omit provably zero releases
  (issues #548/#549, local PRs #665/#667).
- Tail `list.pop()` avoids survivor relocation; other positions repair only the
  suffix (issue #52). Numeric/bool rows use natural 1/2/4/8-byte widths and
  lists omit the dictionary value slot (issue #531).
- Borrowed integer/float/bool list rows use inline reads/writes with unchanged
  bounds reporting. The recorded 3k-line dissimilar diff fell from 265 to
  100 ms (issue #509, local PR #598;
  [measurements](../development/profiling.md#inline-scalar-list-rows)).
- Collection access/removal, equality and string indexing work shipped
  (issues #18–#21). Layout and runtime constraints still follow the
  [reference](../design/toolchain/02-architecture/values.md), not these snapshots.

## B19: Compiler scale and caching

- Cached collection element facts (issue #6), linear declaration/large-enum
  match checking (issues #53/#71), parallel native emission and the large-function
  single-pass allocation fallback (issues #55/#56) shipped. More than 2,048
  branch arguments select the fallback with a code-size/speed tradeoff.
- Operation checking/native IR construction/compilation run across cores with
  byte-identical output for any worker count (local PR #131); lookup, frame
  validation, tail-call preparation and adapter scans also parallelize
  (local PR #136). Declarations resolve once (issue #59, local PR #158).
- Compiler peak memory was measured at about 1.8 KB/source line for checking
  and 2.8 KB for object emission after exact-length syntax lists/40-byte tokens,
  duplicate-body removal and earlier release of tokens/operations
  (issues #422/#54). Whole-program check work is linear in instructions and
  std imports no longer enable per-function whole-program work (issue #57).
- Content-addressed compiled-function reuse shipped (issue #144, local PR #176).
  Buffered cache hashing/shared decoded stencils reduce repeated work
  (issue #316). Whole-object cache validation includes module text/import
  resolution (issue #317 stage 1, local PR #385).
- Generic naming is linear; template/program instance limits and dependency
  depth bounds permit finite declaration chains (issues #428/#430/#435,
  local PR #516). Templates release when no pending lowering needs them,
  reducing generic/ordinary checking memory from 1.92× to 1.37× in the
  recorded 200×200 case (issue #434, local PR #521). Instance-time substitution,
  moved template bodies, template-ID cache keys, symbol-keyed requirements and
  cheaper parser tokens reduced the checking instruction ratio to 1.37×
  (local PR #534).
- Cranelift 0.137's reused regalloc2 context reduced debug compile instructions
  4.2–4.7% in recorded workloads (issues #153/#421, local PR #528).
  Reusing cleared Cranelift Functions reduced ordinary construction allocation
  from 20.3 to 4.9 KB/function
  ([measurements](../development/profiling.md#reusing-function-storage)).
- Batched codegen results preserve deterministic output with fewer messages
  (local PR #368). Mimalloc removed glibc arena-lock contention, so the measured
  16-thread run was no slower than eight (issue #315, local PR #386).
  Linker preference chooses mold/lld while preserving overrides
  (issue #314, local PR #367).

## B19: Profiling and concluded investigations

`--features profile` exposes hotpath timing/calls/allocations per phase
(issue #150, local PR #188); finer lowering/ownership/native rows and isolated
compile_bench workloads supported investigations #301–#305 (local PR #343).

| Investigation | Recorded outcome |
| --- | --- |
| #301 | Avoiding 128-bit slot split/join shifts halved AOT instructions and reduced register-allocation work 70% (local PR #351). |
| #303 | Borrowed loan-graph operations and liveness-buffer reuse cut ownership-check instructions 40% (local PR #352). |
| #302 | Shared token/nominal names first cut checking instructions 8.8% and allocations 25% (local PR #357); shared structural/callable children and FxHash tables brought the broader check-workload reduction to about 20% (local PRs #383/#389). Investigation closed. |
| #305 | The mixed-workload checking ratio was attributed to cache/TLB misses, not algorithmic work. |
| #304 | Warm-cache work was measured at roughly 45% frontend and 18% linking; [linking experiments](../proposals/faster-linking.md) record levers. |
| #59 | Parallel parsing, speculative lowering and declaration parallelism were evaluated but not adopted: small gains and higher peak memory. Declaration reuse landed. |
| #321 | Mimalloc and skipped command-line teardown were retained; arenas/bump allocation were measured and not adopted (local PR #388). |
| #317 | Stored syntax trees/separately compiled module artifacts did not pay; the investigation identified imported-interface lowering as a prerequisite for a useful lowered-module cache (local PR #392). See [module-cache investigation](../development/module-cache-investigation.md) and [module-edit baselines](../development/module-edits.md). |
| #436 | Substitution-environment lowering offered at most 2.6% and was set aside. Instance registration/header tokens explain the remaining generic gap. Closed. |
| #437 | Code placement affects SIMD measurements; retained [placement research](../proposals/columnar-list/placement-research.md), no compiler fix. Closed as investigated/deferred. |
| #440 | Cranelift alias regions gave no gain and no sound general partition; reference targets can span frames/rows/fields/payloads. `test_memory_order` guards against unsound regions (local PR #529). |
| #441 | Cranelift 0.137 dead-store elimination was qualified against call/atomic cross-thread handoffs; release concurrency/coverage tests and >1 GiB frame diagnostics guard the contract (local PR #530). |

These outcomes are not active claims. Any revisit follows **B19** in the
[backlog](../backlog.md), with new evidence.

## B19: Traps and arithmetic lowering

One object-wide trap-site table and cold function check blocks reduced the
recorded 200×200 object size 34% and single-core emission 9%
(issue #420, local PR #505). Debug `cmp; jcc` overflow checks reduced that
benchmark's text 5.5% (issue #425, local PR #525). Cranelift 0.137 fuses unsigned
add/all multiply checks and keeps pooled float-mask lanes in registers
(issue #439, local PR #528).

PC-relative trap tables and executable `ud2`/SIGILL resume-to-report helpers
reduced recorded text 14% debug/11% release and compile instructions 7–9%
(issue #425, local PR #531). Libraries/fuzz/coverage retain helper calls.
Issue #425 was closed as deferred to upstream changes, not as proof that every
check/frame/spill cost disappeared. The [runtime reference](../design/toolchain/02-architecture/runtime.md)
owns the contract; upstream revisit gates are in **B19** of the [backlog](../backlog.md).

## B23: SIMD

[Automatic integer-column filter/count](../proposals/columnar-list/simd-counts.md)
uses baseline four-lane i32/u32 SIMD with scalar tails in optimized builds.
One/two total comparisons joined by `and`, inclusive/exclusive ranges, reversed
operands and up to two immutable same-type scalar captures are supported.

The initial [std.simd](../design/stdlib/simd.md) module provides four concrete
numeric vector shapes, masks, checked/wrapping arithmetic, selection, safe list
access and scalar fallback. [Bounded lowering/local promotion](../proposals/std-simd/implementation.md)
avoid repeated materialization in supported kernels. Direct integer wrapping
reductions avoid the spill reproduced in issue #444. These are delivered
subsets, not automatic packed generic arrays or a backend switch. Remaining
extensions stay **B23/B76** in the [backlog](../backlog.md).

## B74: Collection hashing

Collections use SipHash-2-4 with a per-process OS key (issue #329), and record
traversal performs linear field-offset work per pass (issue #330). This does
not mean user-defined structural key types shipped; that design remains
**B74** in the [backlog](../backlog.md).
