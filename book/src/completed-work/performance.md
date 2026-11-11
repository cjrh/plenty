# Completed compiler performance and SIMD work

[Completed-work index](../completed-work.md) · [Remaining work](../backlog.md)

These are delivery/investigation records, not current benchmark guarantees or
an optimization queue. Workloads, methods and current measurements belong to
[profiling](../development/profiling.md); remaining B19/B23/B74 scope belongs
only to the [backlog](../backlog.md).

## B19: Generated code and storage

A successful list/view bounds check now proves that incrementing the same
`i64` index by one cannot overflow, while its dominance chain remains valid
(issue #930). This removes three instructions per n-body velocity pair and
reduces the complete benchmark from 2.050B to 1.996B instructions at one million
steps, with identical output. Bounds checks and non-dominating overflow checks
remain.

Dictionary hash probes read only the key instead of materializing an unused
key/value row (issue #931). The isolated hit path saves 29 instructions per
update, and k-nucleotide at fasta 50,000 saves about 3% of instructions with
identical output. Other helper costs remain; wall-time improvement was not
established on the shared machine.

Building a string from a byte range (`Bytes.to_text_range`) reads the buffer's
address and length with one helper, skips slice clamps when two unsigned
comparisons show the bounds in range, converts through a register-only helper
instead of the dispatcher, and packs an ASCII key of at most seven bytes
without a loop (issue #931, `experiments/key-construction/`). One key costs 105
instructions instead of 173; k-nucleotide at fasta 50,000 falls from 1.403B to
1.282B instructions (8.6%) with identical output, fasta by 1.3% and regex-redux
by 0.4%. The in-range slice path applies to every list, view and byte slice
with a written bound.

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
  (issue #395; [measurements](../development/profiling-match-results.md#dense-match-dispatch)).
  String matches with 16+ literals binary-search one sorted table
  (issue #565; [measurements](../development/profiling-match-results.md#string-match-dispatch));
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
  See [flat-layout evidence](../proposals/completed/flat-class-layout.md).
- Release builds inline small constructors/`__init__`/`new`, avoid release of
  fields initialized first and omit provably zero releases
  (issues #548/#549, local PRs #665/#667).
- Tail `list.pop()` avoids survivor relocation; other positions repair only the
  suffix (issue #52). Numeric/bool rows use natural 1/2/4/8-byte widths and
  lists omit the dictionary value slot (issue #531).
- Borrowed integer/float/bool list rows use inline reads/writes with unchanged
  bounds reporting. The recorded 3k-line dissimilar diff fell from 265 to
  100 ms (issue #509, local PR #598;
  [measurements](../development/profiling-allocation-results.md#inline-scalar-list-rows)).
- Collection access/removal, equality and string indexing work shipped
  (issues #18–#21). Layout and runtime constraints still follow the
  [reference](../design/toolchain/02-architecture/values.md), not these snapshots.

### Byte method access (issue #741)

`Bytes.get` and `Bytes.set` use narrow native helpers rather than the wide
collection dispatcher. The helpers share the dispatcher's strict index checks;
negative indices remain invalid for these methods. Slot/field receivers remain
borrowed through the call, while owned temporaries retain their normal cleanup.
The earlier ownership-copy and shared-inlining changes (issues #744 and #770)
already removed the original retain/release pairs and out-of-line Plenty method
calls. This change removes the remaining per-access argument/result scratch
blocks without changing public signatures or diagnostics.

The byte kernels (`experiments/kernels/bytes/README.md`) record the
scalar C comparison, optimized CLIF variants, instruction attribution and
whole-program/compilation measurements. The optimized caller kernel fell from
62 to 38 instructions per byte; the quick reverse-complement input used 25.3%
fewer instructions with identical
output. Caller-site bookkeeping, repeated checks and unused owner-word loads
remained (issues #842–#844); the result is not a claim of C-equivalent code.

### Loop-cached byte access (issue #843)

A loop that cannot resize or replace a `Bytes` buffer reads its length and
first-byte address once and accesses bytes directly; see the
[collections architecture](../design/toolchain/02-architecture/collections.md).
An index the loop does not prove in range keeps one unsigned comparison, and a
failing index traps through the method with unchanged messages and sites. The
byte kernel fell from 38 to 7 instructions per byte (C: 4); the remaining
difference is the range loop's separate down-counter and index. The quick
reverse-complement input used 81.5% fewer instructions with identical output.
In cached loops the caller scopes and unused owner-word loads (issues #842 and
#844) leave the loop with the calls; both were later removed from uncached
calls as well.

### Caller sites in inlined byte access (issue #842)

In optimized builds (the default), a user function that inlines `Bytes.get`,
`Bytes.set`, `Bytes.len` or `Bytes.push` no longer saves and restores the
thread-local caller around each call. The inlined trap records name the user
call site themselves; see [trap sites](../design/toolchain/02-architecture/runtime.md#trap-sites).
Trap messages and caller notes are unchanged. A loop that pushes to its buffer,
and so keeps the method calls, fell from 68 to 45 instructions per byte, with
no `plenty_caller_enter` call left in it. The quick reverse-complement input,
whose reversal `while` loop still calls the methods, used 14.3% fewer
instructions with identical output. Std bodies that call, loop or run
destructors still use the thread-local context.

### One-word reference reads (issue #844)

A read through a reference of a one-word value, such as the `Bytes` owner the
inlined `get` and `set` read through `self`, loads only the low slot word;
Cranelift kept the dead high word of a two-word load. Loops that keep the
`Bytes` method calls lose about one instruction per method call on top of
issue #842's change: the pushing byte kernel fell from 45 to 42 instructions
per byte. Other one-word reads through references benefit likewise. The fasta
benchmark used 1.0% fewer instructions and quick reverse-complement 0.09%
fewer; see `experiments/kernels/bytes/README.md`.

### k-nucleotide counting-loop investigation (issue #743)

The original issue was split into issues #744–#750, with follow-ups #755,
#757, #761 and #762. Their fixes or investigation decisions are delivered:

| Cost in the original report | Delivered outcome | Local PRs |
| --- | --- | --- |
| Count pairs around borrowed collection operands | Lookup operands stay in place; last reads move after their loans end; inserts adopt owned operands. | #939, #950, #971 |
| Atomic final release | A sole owner skips the locked decrement. The proposed plain retain was rejected as unsound because uncounted borrows can retain concurrently. | #938 |
| Thread-local counting | Design recorded; the single-threaded experiment removed locked counts without a measurable cycle gain on Zen 4, so it was closed unmerged. | #937, #944 |
| Generic key hashing | Top-level text/bytes and scalar keys use direct SipHash-1-3 paths. | #945 |
| Rehashing on a miss, growth and collisions | `dict.get_or_insert` performs one lookup; buckets retain hash bits for growth and reject unequal hashes before comparing keys. | #954, #956, #976 |
| Text-range allocation and validation | Builders fill uninitialized storage; ASCII ranges avoid separate UTF-8 validation and scalar counting; matched sum values avoid rebuilding wrapper payloads. | #943, #953 |
| Trap-site bookkeeping | Code-free releases and proven read-only std accessors omit caller-site bookkeeping. | #948 |

Both serial and parallel k-nucleotide benchmarks use `get_or_insert` for
counting. This closes the original investigation, not every possible counting
optimization: required UTF-8 validation, eager default construction and
bookkeeping for helpers that can trap retain their contracts. See
[dictionary operations](../design/facilities/01-builtins/collections.md),
[sole-owner release](../design/toolchain/02-architecture/memory.md#sole-owner-release)
and the [rejected counting experiment](../proposals/completed/thread-local-reference-counts.md#measured-result).
The child issue/PR records retain their measurements against different intermediate
revisions; their percentages must not be added or treated as a fresh combined
speedup. The original serial-versus-seven-thread wall-time ratio is likewise
not a current per-core comparison.

## n-body structural feasibility (issues #929 and #930)

The investigation in `experiments/nbody-structural/README.md` executes and checks
fixed-state scalar/vector IR against Plenty, C and Rust. At main `c7ca5a95`, one
million steps take 1,996M instructions in current Plenty, 857M in a fused-source
probe, and 673M with explicit five-body scalar locals. Cranelift vector IR takes
488M on baseline SSE2 and 389M with AVX enabled; native Rust takes 230M.
In one pinned timing session, current Plenty takes 102ms, the AVX prototype
38ms and Rust 28ms. These are feasibility probes, not shipped compiler gains.

Full state is bit-identical across the Plenty/C/IR probes over short and long
runs, with 2,560 additional randomized C/IR checks. Public Plenty SIMD still
loses the target shape through lane-extraction calls and aggregate storage.
The study provides complete loop instruction ledgers, reproducible generators,
counter observations, timing ranges and limitations. It supports scalar
replacement plus bounded expansion and native SIMD continuity; neither array
syntax alone nor indiscriminately higher inlining limits establishes that result.
The production benchmark, compiler and baseline ISA remain unchanged. Outstanding
implementation scope belongs to B19/B23/B83 in the [backlog](../backlog.md).

## Guarded numerical state (issue #930)

Optimized builds now automatically expand bounded inner ranges and promote small
numerical list/record cells across an outer loop, with size/fact guards and the
original loop as fallback. The [implemented limits and safety argument](../design/toolchain/02-architecture/numerical-loops.md)
cover preserved allocation failures, aliasing, floating expression order and
empty ranges. No source rewrite is needed for the original n-body benchmark.

`experiments/fixed-state/README.md` and its retained measurements report one
million steps falling from 1,995.713M to 702.713M instructions (64.8% fewer), and
104.89ms to 51.82ms in one rotating, pinned timing session. Native Rust rerun in
that session takes 229.636M instructions and 28.04ms. The fast loop has 280 scalar
FP instructions, 254 stack moves, 153 register copies, ten zeroing instructions
and five control instructions; no list accesses or checks remain inside it.

The cost is 12.0% more n-body compilation instructions and `advance` growing
from 6,259 to 14,286 bytes, including its fallback. Limits bound expansion,
promoted cells and added locals. Complete-state checks include the original
initial state and eighty randomized cases; fallback and allocation-failure
checks retain their original behavior. Broader aggregate eligibility, automatic
SIMD packing and backend register use remain issue #930 in the [backlog](../backlog.md).

## Value bounds and loop conditions

Instruction counts (`instructions:u`, x86-64, `--release`) against `28cb7214`,
with identical output:

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| spectral-norm 2000 | 7,044,670,022 | 2,724,670,912 | −61.3% |
| mandelbrot 2000 | 2,813,304,987 | 2,096,572,752 | −25.5% |
| fasta 2,500,000 | 2,793,851,083 | 2,186,823,727 | −21.7% |
| n-body 500,000 | 351,380,217 | 345,880,163 | −1.6% |

The other Benchmarks Game programs change by at most 0.4%. The lowerings are
described under [value bounds](../design/toolchain/02-architecture/emission.md#value-bounds),
[floor division by a positive divisor](../design/toolchain/02-architecture/emission.md#floor-division-by-a-positive-divisor),
[loop counter checks](../design/toolchain/02-architecture/emission.md#loop-counter-checks)
and [constants in loops](../design/toolchain/02-architecture/emission.md#constants-in-loops).
Kernels and per-pass attribution are in `experiments/kernels/value-bounds/`;
issues #932, #933 and #934 record the remaining causes.

## Indexed loop conditions, range loop tests and `bytes` reads (issues #932 and #933)

A loop whose condition indexes a view or a cached list (`while limits[k] < r:`)
is rotated and its unit step has no overflow check; a unit-step `range` loop
whose body reads its variable tests it against the stop and keeps no
countdown; and a shared list's cached length and rows are read once before the
outermost loop that follows the binding. `len`, indexing and iteration of a
`bytes` value are inline loads, with a constant length for a known literal,
and no longer end a loop's cached `Bytes` header. Instruction counts against
`f0a789a7`, with identical output:

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| fasta 2,500,000 | 2,186,826,930 | 1,647,914,707 | −24.6% |
| n-body clist 500,000 | 1,283,860,195 | 1,193,362,116 | −7.0% |
| fannkuch-redux 9 | 165,231,771 | 160,557,101 | −2.8% |
| n-body 500,000 | 345,878,086 | 352,880,013 | +2.0% |

The other Benchmarks Game programs change by at most 0.2%. In n-body the
optimized IR of `advance` is smaller (14 fewer subtractions, 3 fewer
compares) and its inner loops are equal or shorter, but the expanded step
loop is 14 instructions longer after register allocation. The lowerings are described under
[loop layout](../design/toolchain/02-architecture/emission.md#loop-layout),
[loop counter checks](../design/toolchain/02-architecture/emission.md#loop-counter-checks)
and [`bytes` values](../design/toolchain/02-architecture/emission.md#bytes-values).
Kernels and attribution are in `experiments/kernels/indexed-conditions/` and
`experiments/kernels/bytes-literals/`.

## Known returns and reference parameters (issues #936 and #937)

A return of an inlined call that built `Ok` or `Some` jumps past the caller's
`?` test, and the returns that built the failure variant go to the failure
path, so `rng.next()?` on a small method has no tag test. A reference
parameter is split into its address and tag shift once per function, which
lets Cranelift forward a stored field to the read after it, and a signed
remainder by a positive constant has a recorded bound. Instruction counts
against `5f43644d`, with identical output:

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| n-body 500,000 | 353,093,346 | 328,092,973 | −7.1% |
| n-body unchecked 500,000 | 353,093,216 | 328,093,105 | −7.1% |
| fasta 2,500,000 | 1,648,133,194 | 1,608,132,163 | −2.4% |
| fannkuch-redux 9 | 160,775,453 | 160,372,355 | −0.3% |
| k-nucleotide (250k input) | 6,374,860,845 | 6,363,526,628 | −0.2% |
| reverse-complement (2.5M input) | 655,885,665 | 656,760,580 | +0.1% |

The other Benchmarks Game programs change by less than 0.01%. The fasta
generator still runs two overflow checks and a floor fixup per number,
because the range of the `seed` field is unknown: issue #937 stays open for
field value bounds. The lowerings are described under
[known returns](../design/toolchain/02-architecture/emission.md#known-returns),
[reference parameters](../design/toolchain/02-architecture/emission.md#reference-parameters)
and [value bounds](../design/toolchain/02-architecture/emission.md#value-bounds).
Kernels and attribution are in `experiments/kernels/field-rng/`.

## Overflow flags fused into branches (issue #934)

Cranelift 0.137 branched on the flags of `uadd_overflow` and the multiplies
only; a signed checked add or subtract was `add; seto; test; jne`. The
[Cranelift fork](../development/workflow.md#cranelift-and-regalloc2-forks)
takes the upstream rules for `sadd_overflow`, `ssub_overflow` and
`usub_overflow` (Wasmtime #14717), so each kept check is `add; jo` or
`sub; jb`. On x86-64 the compiler no longer replaces unsigned subtraction and
debug-build constant steps with an `icmp` on the result (issue #425); AArch64,
whose backend has no such rules, keeps that form. Instruction counts against
`5e7906ad`, with identical output:

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| reverse-complement (2.5M input) | 656,759,429 | 603,009,387 | −8.2% |
| fasta 2,500,000 | 1,608,130,869 | 1,521,464,243 | −5.4% |
| fannkuch-redux 9 | 160,371,176 | 152,285,422 | −5.0% |
| regex-redux (250k input) | 2,541,075,263 | 2,505,730,957 | −1.4% |
| binary-trees 14 | 1,576,540,003 | 1,562,091,571 | −0.9% |
| k-nucleotide (250k input) | 6,365,167,778 | 6,326,091,692 | −0.6% |
| mandelbrot 2000 | 2,092,776,724 | 2,085,276,431 | −0.4% |
| n-body 500,000 | 328,091,878 | 327,091,479 | −0.3% |
| n-body clist 500,000 | 1,193,573,891 | 1,203,573,620 | +0.8% |

spectral-norm is unchanged. Program text shrinks 0.04% to 0.40% and compiler
instructions 0.2% to 0.3%. In n-body clist every innermost loop of `advance`
has the same per-pass count and the optimized IR is identical; the 20
instructions per step are 5 moves and a jump in outer-loop code whose
register assignment changed.

## Loop-exit blocks kept off the loop's allocation

A loop whose bounds-check failure path calls a reporting function before it
traps ran extra register moves on every pass. regalloc2 gave the failure
block the loop's depth, because Cranelift hands it over inside the loop's
span of the block order, and a value used by that call shared one bundle with
its uses in the loop, so the call's argument register became a requirement of
the whole loop. The
[regalloc2 fork](../development/workflow.md#cranelift-and-regalloc2-forks)
gives a block without successors depth 0 and, when it is entered from inside
a loop, gives each value's ranges in it a bundle of their own: the move into
the argument register runs on the exit edge. The fork's
`docs/cold-exit-block-splits.md` has the reproductions. Instruction counts
against `761f7813`, with identical output:

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| n-body 500,000 | 327,092,611 | 323,092,572 | −1.2% |
| reverse-complement (2.5M input) | 603,009,834 | 602,718,127 | −0.05% |
| mandelbrot 2000 | 2,085,277,533 | 2,084,277,546 | −0.05% |
| regex-redux (250k input) | 2,505,731,496 | 2,505,963,327 | +0.01% |
| n-body clist 500,000 | 1,203,574,655 | 1,234,574,648 | +2.6% |

fasta, spectral-norm, binary-trees and fannkuch-redux are unchanged, and
k-nucleotide is within its run-to-run spread. The clist velocity kernel
(`experiments/kernels/clist/`) falls from 74 to 70 instructions per pass; the
other kernels are unchanged. Compiler instructions rise 0.07% to 0.22%.

n-body clist loses 62 instructions per step in `advance`'s velocity nest. The
unpatched allocator counted each failure block's use of the lengths of `dy`,
`dz` and `mag` at the inner loop's depth, which kept them in registers ahead
of a competing value; with that use at depth 0 they are spilled. The lengths
are reloaded on every outer pass and need not be (issue #943).

## B19: Compiler scale and caching

- Warm single-file executable hits skip bundled-runtime extraction (issue #699).
  The measured no-op shebang median fell from 16.35 to 6.56 ms while preserving
  source/toolchain/native-input checks. See the
  [startup measurements and method](../development/script-startup.md).

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
  ([measurements](../development/profiling-allocation-results.md#reusing-function-storage)).
- Batched codegen results preserve deterministic output with fewer messages
  (local PR #368). Mimalloc removed glibc arena-lock contention, so the measured
  16-thread run was no slower than eight (issue #315, local PR #386).
  Linker preference chooses mold/lld while preserving overrides
  (issue #314, local PR #367).

## Compiler architecture assessment

A source-based [refactoring proposal](../proposals/completed/compiler-refactoring.md)
records the compiler boundary assessment, file/symbol maps, alternatives and
validation gates. Its six steps are delivered: structured diagnostics (issue
#691), scoped recorder and teardown state (#692), the `op` and syntax-layer
splits (#693, #694), grouped pipeline state (#695) and a frontend lookup
profile that changed no representation (#696). The legacy stack syntax is
removed (#698). No speedup is claimed; remaining scope stays in the backlog.

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

The Linux foreign-handler fixture repeats signal delivery until the tested read
completes, instead of assuming a scheduling delay puts the reader in the syscall
(issue #689). Restarting reads receive a pipe byte after repeated signals;
non-restarting reads must report `EINTR`, with a bounded watchdog. Delayed-entry
regressions retain the later checked-overflow report in debug and optimized builds.
This changes test synchronization, not runtime handler behavior.

## B23: SIMD

Numeric `std.simd` constructors and `to_tuple()` now preserve native lanes through
unpacking on optimized x86-64 builds (issue #929). Unborrowed matching numeric
tuples used by destructuring can remain in vector SSA; calls and addressable
values retain the ordinary representation. The scalar fallback and public ABI
are unchanged.

The retained `experiments/simd-boundaries/` probes compare unchanged source at
main `56365416` with this lowering. The projection loop falls from 129 executed
instructions per iteration (including caller-context helpers) to 9, matching
C's loop. At one million steps, the public fixed-state x/y n-body probe falls
from 2,024.686M instructions to 545.684M, with same-session median time falling
from 253.75 ms to 51.12 ms. All 35 state fields and both energy lines are exact
across nine step counts. These are explicit-SIMD source probes: the original
list-based benchmark remains at 1,995.708M instructions. Automatic aggregate
scalar replacement and bounded loop expansion remain separate work.


Float vectors now provide allocation-free `sqrt()` with scalar fallback and
baseline x86-64 packed lowering (issue #929). IEEE edge cases and chained
vector-register operations are tested. Public list-transfer overhead still
prevents the n-body magnitude path from approaching the packed-pointer probe;
this adds a numerical primitive, not a benchmark speedup.

List loads and stores are packed on optimized x86-64 builds (issue #929): one
unaligned vector move after the scalar bodies' bounds test, with `Nothing` or
`False` for a span that does not fit. An unborrowed `Option[Vector]` local,
`match` and `unwrap()` keep the loaded vector native. The retained
`experiments/simd-transfers/` kernel falls from 4,194 executed instructions per
two pairs at main `d6ed6f91` to 79 with nested `match`; scalar source runs
36 and C 16. Header reloads, length arithmetic and bounds tests for each
transfer account for 44 of the 79. An n-body variant using the transfers falls
from 11,304M to 985M instructions at 500,000 steps and remains above the
scalar source's 323M, which alone qualifies for guarded numerical state; the
benchmark source stays scalar and every benchmark binary is unchanged.

[Automatic integer-column filter/count](../proposals/columnar-list/simd-counts.md)
uses baseline SSE2 SIMD with scalar tails in optimized builds: 16 lanes for
8-bit, 8 for 16-bit and 4 for 32-bit integer columns (issue #904).
One/two total comparisons joined by `and`, inclusive/exclusive ranges, reversed
operands and up to two immutable same-type scalar captures are supported.

The initial [std.simd](../design/stdlib/simd.md) module provides four concrete
numeric vector shapes, masks, checked/wrapping arithmetic, selection, safe list
access and scalar fallback. [Bounded lowering/local promotion](../proposals/std-simd/implementation.md)
avoid repeated materialization in supported kernels. Direct integer wrapping
reductions avoid the spill reproduced in issue #444. These are delivered
subsets, not automatic packed generic arrays or a backend switch. Remaining
extensions stay **B23/B76** in the [backlog](../backlog.md). This delivers the
initial SIMD stage of the [memory and parallelism assessment](../proposals/memory-parallelism-and-simd.md),
which remains partly implemented for its public allocator design.

## B74: Collection hashing

Named enums with supported key payloads now use structural hashing in sets and
dictionary keys. Variant tags and all payload fields participate; nested named
enums, tuples and classes, including custom cleanup, are supported. Anonymous sums with supported members also qualify; their active member tag
and structural value define identity. Floats, boxes and recursive types remain excluded. These general key types use keyed runtime hashing. Supported finite enum and tuple domains with at
most 256 values use the bitset specialization described here. See [collections](../design/facilities/01-builtins/collections.md).


Collections use SipHash-2-4 with a per-process OS key (issue #329; issue #747
later moved to SipHash-1-3 with direct `str`, `bytes` and scalar key paths), and record
traversal performs linear field-offset work per pass (issue #330). The built-in `KeyType` constraint now exposes the same key admission rule on
functions, methods, classes and enums; see [generics](../design/language/07-generics/index.md#keytype).
Custom-cleanup keys use the same structural identity without making their
cleanup obligations copyable.

Class keys hash their fields rather than their addresses. Move-only class keys,
and enums containing them, transfer on insertion and owned extraction; lookups
and views borrow them. Set algebra and key-view `to_set()` create independent
copies, and affine `keys_snapshot()` requires an owned temporary, matching
`values_snapshot()`. Keys containing custom cleanup support borrowing,
transferring updates, in-place filtering and owned extraction. Non-mutating
algebra, `to_set()` and explicit copying reject them. Duplicate insertion keeps
the existing key owner and destroys the incoming owner. Regression tests cover
nested cleanup, generic boundaries, duplicate ownership and allocation failures.

### Small enum sets

The initial specialization used one membership word for payload-free named enums
with up to 64 variants, without entry/order/hash buffers or entropy reads.
The [wider implementation](#wide-enum-bitsets) extends this to 256. The collection owner
still allocates once. Membership, length, insertion/removal, same-type set
relationships and algebra use bit operations. Shared iteration uses immutable
canonical enum values, while normal source loans remain enforced. Mixed
operations with dictionary key views retain their existing semantics.

Allocation-failure tests cover the remaining owner allocation and allocation-free
mutation; boundary tests cover 1, 63, 64 and 65 variants. These initial
measurements cover payload-free enums at revision `f4aba2de`. Reproduction at
that revision is
`cargo build --locked --bin plenty` (without instrumentation features), then
`python3 scripts/bench_enum_sets.py target/debug/plenty --output /tmp/enum-sets.json`:
the same first 64 variants are exercised in a 64-variant bitset and a 65-variant
hash set. It checks equal results and alternates execution order. Native programs
use `--release`; measurements are observations, not performance assertions.

Measured on x86-64 AMD Ryzen 7 7840HS, with 200,000 rounds and median of five
runs per case. Each lookup round probes 64 values; each mutation round adds and
removes all 64. Union combines two disjoint 32-member sets; copy duplicates a
32-member set. These timings describe these workloads on this machine, not a
general speed guarantee.

| Workload | Bitset (seconds) | Hash fallback (seconds) | Speedup |
| --- | ---: | ---: | ---: |
| Lookup | 0.081969 | 0.357319 | 4.36× |
| Add and discard | 0.339045 | 1.598463 | 4.71× |
| Union | 0.012670 | 1.210755 | 95.56× |
| Copy | 0.013672 | 0.461085 | 33.72× |

### Finite payload enum sets

The finite-payload stage (`82bd5231`) extended bitsets to named enums and tuples
whose complete domains contain at most 64 values, recursively composed of booleans and named enums/tuples.
The compiler checks sums and products against the limit before generating
canonical values and variant offsets. Membership encodes the active payload with
mixed-radix indexing; algebra still operates directly on the mask. Immutable,
16-byte-aligned metadata includes nested inline records, preserving stable shared
references without runtime caches or allocations. The runtime type descriptor
now carries an optional finite-domain descriptor.

Tests cover nested multi-field enum/tuple values, every member of a 64-value
domain, the 65-value hash fallback, singleton/Option payloads, mixed dictionary views,
copying and iteration after source destruction, and allocation failure. The
benchmark at that revision also supports `--payload` to compare a six-boolean variant (64 values)
with the same variant plus an unused extra case (65 values). Integer/string
payloads and classes keep hash storage; the [wider implementation](#wide-enum-bitsets)
extends the finite-domain threshold to 256.

On the same AMD Ryzen 7 7840HS, 200,000 rounds and medians of five runs with
`--payload` produced the following observations. The workload counts match the
[payload-free benchmark](#small-enum-sets), but each key now carries six boolean fields.
These are measurements of these workloads, not general speed guarantees.

| Workload | Finite bitset (seconds) | Hash fallback (seconds) | Speedup |
| --- | ---: | ---: | ---: |
| Lookup | 0.584369 | 1.961403 | 3.36× |
| Add and discard | 1.594221 | 7.373249 | 4.62× |
| Union | 0.011405 | 4.927148 | 432.02× |
| Copy | 0.015694 | 1.980686 | 126.21× |

### Wide enum bitsets

Finite enum and tuple domains select their mask width from the complete type
domain: one 64-bit word for 1–64 values, two words for 65–128, and four words for
129–256. Operations dispatch to the selected width, including scalar membership,
popcount and algebra for the smallest domains. Clearing or draining preserves the
selection; sparse membership never narrows it. All alternatives fit within the
existing insertion-order storage, so collection owners do not grow. The canonical payload-free
tag table grows from 64 to 256 entries (one shared 4 KiB table); finite payload
metadata remains proportional to domain size and inline layout. Wider masks do
not change key admission or dictionary storage. No target-specific SIMD is needed.

Boundary tests cover 64/65, 128/129 and 256/257 values, including hash fallback.
Randomized membership and mixed-end iteration tests exercise every word; native
checks cover nested enum/tuple payloads, set algebra, dictionary views, copied
values, owner failure and allocation-free mutation.

Requested heap storage measured with the runtime's actual buffer capacities
(excluding allocator overhead and immutable type metadata):

| Members | Bitset bytes | Payload-free hash-set bytes |
| ---: | ---: | ---: |
| 0 | 176 | 176 |
| 4 | 176 | 496 |
| 64 | 176 | 4,272 |
| 128 | 176 | 8,368 |
| 256 | 176 | 16,560 |

Both owners are 176 bytes on this x86-64 build. The hash comparison uses a
257-variant enum and inserts the same tags without pre-reservation. Payload
hash rows can require more storage; canonical payload data is shared per type,
not part of these per-owner heap figures. For example, one eight-boolean variant
emits 256 × 16-byte canonical slots plus 256 × 128-byte payload records: 36 KiB
of shared value data, plus descriptors and alignment. This can outweigh the
heap saving for a small number of sparse sets; the optimization trades static
data for faster operations and smaller per-instance storage. Reproduce with
`cargo test -p plenty-runtime wide_enum_set_storage_measurements -- --nocapture`.

The benchmark accepts `--size 64|128|256`, `--payload`, `--members`, and
`--workloads`. Without a baseline compiler it adds unused variants to force a
257-value hash domain. `--baseline-compiler` instead compares identical programs
against an older compiler; this also checks the existing 64-value specialization.
Every result is checked, execution order alternates, and JSON records individual
wall and child CPU times as well as medians.

The wide-domain comparison uses the pre-change 64-value compiler, native
`--release` compilation without runtime instrumentation, and an AMD Ryzen 7
7840HS. Reported ratios use median user-plus-system CPU time over five runs.
Other work was running on the host; CPU time excludes scheduling waits but
cannot remove effects on clock speeds and caches. These are workload-specific
observations, not speed guarantees.

For the full-domain cases, 50,000 rounds each probe all values, add and remove
all values, union two disjoint half-domain sets, or copy a half-domain set.
Payload cases use seven or eight boolean fields. Sparse cases use four values
spread across the 256-value domain (two per input set) and 1,000,000 rounds.
Reproduce a full-domain case with:

```sh
cargo build --locked --bin plenty
python3 scripts/bench_enum_sets.py target/debug/plenty \
  --baseline-compiler /path/to/pre-change/plenty \
  --size 256 --payload --rounds 50000 --runs 5 --output /tmp/wide-enums.json
```

Use `--members 4 --rounds 1000000` for sparse cases, and omit `--payload` for
payload-free enums.

Speedups over the older hash-backed implementation:

| Domain / workload values | Lookup | Add/discard | Union | Copy |
| --- | ---: | ---: | ---: | ---: |
| 128, payload-free | 3.75× | 4.23× | 136.24× | 115.01× |
| 128, boolean payload | 4.15× | 4.46× | 696.99× | 410.92× |
| 256, payload-free | 3.56× | 4.12× | 198.95× | 192.41× |
| 256, boolean payload | 4.38× | 4.48× | 1342.00× | 849.87× |
| 256, four payload-free values | 2.91× | 3.64× | 4.47× | 8.90× |
| 256, four payload values | 3.64× | 3.79× | 26.00× | 22.92× |

[Raw measurements](data/enum-bitsets-256.json) retain every wall/CPU sample,
medians, settings, compiler hashes and the 64-value comparison. The gains at
both wider sizes and in sparse cases support the 256-value cutoff.

The 64-value comparison uses seven alternating runs: 2,000,000 rounds for union
and copy, 200,000 for lookup and mutation. Relative throughput changes against
the old scalar implementation were lookup +0.3%, add/discard +3.5%, union +1.4%, copy +163.6%. These are measured
medians, not guarantees of changes this small. Width is encoded once in the
storage alternative, with scalar operations for small domains. Copying reuses
the validated representation and allocates only the new owner, avoiding storage
reclassification and unnecessary reservation or membership counting.

## Selected-field column zip (issue #888)

Unadapted `clist` columns zipped together now use a shared cursor over their
shorter remaining range, with column pointers prepared once. Named and partially
advanced owned column plans preserve their positions; callbacks, filtering and
lent iterator state retain the general left-first pull path. The source owner
loans remain active throughout consumption.

The retained C/Plenty and IR probes in `experiments/kernels/clist-zip/`
attribute internal pending options, nested pull control and duplicate counters.
On the selected-field kernel, the static innermost-loop instruction span drops
from 43 to 8 for explicit zip and from 43 to 10 for zip/fold; indexed access stays
at 8 and scalar C at 7. These spans are not elapsed-time speedups. See the probe
for traced paths, whole-program instruction counts and remaining limits.

## Conditional callback captures (issue #891)

General call-site inlining now reuses the immutable scalar capture snapshots
already maintained for leaf callbacks. A conditional fold over two `clist`
columns no longer reloads the closure address and captured threshold each row;
its standalone and actual inlined loops both drop from 10 to 8 instructions,
matching the explicit column loop. Scalar C uses 7 because it folds a field
load into the floating addition.

The retained probes in `experiments/kernels/scalar-captures/` compare isolated
IR variants before the compiler change and verify snapshot movement, branch
assignment, closure replacement, mutable and borrowed captures, and float edge
cases. Whole-program instructions fall from 44,158,729 to 40,881,853 with the
same checksum. This is an instruction-count result, not an elapsed-time claim.
