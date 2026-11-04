# Checked-default validation

Branch-scoped validation for local PR #886. This records implemented adoption and
bounded evidence, not a merge, universal ABI certification or performance
superiority. The [proposal](../proposals/checked-by-default.md) records the design;
[implementation and validation](../proposals/checked-by-default/implementation.md)
records its integration history.

## Compared contracts

The frozen baseline is `8e8fccd9`: **main + the common #712 Unit-verifier fix**, not
pristine or current main. The frozen candidate is `830763c9`. Compiler, runtime,
flags and native-tool provenance were authenticated separately for each role.
Later controller repairs and benchmark-template adoption did not rebuild or
replace these comparison compilers.

| Mode | Compiler | Arithmetic contract |
|---|---|---|
| T | Baseline | Original unadorned trapping arithmetic |
| C | Baseline | Original explicit checked recovery |
| I | Candidate | Implicit function-level checked arithmetic |
| U | Candidate | Explicit trapping `unchecked:` |

T/U and C/I compare matching behavior. Trap-process and handled-error timings
are not interchangeable. Operators produce plain successful values; functions
and callbacks independently declare their public `Result` contracts.

## Semantic and native qualification

The final common-fix native matrix passes 124 checks, 124 native builds, 284 exact
runtime cases, six optimized call-retention gates and 24 cleanup cases. All 16 full
1000/4000-unit corpus configurations compile and execute their designated
checksum entrypoints: **1004057** and **16016057**. Four C/I calling-thread
zero-allocation intervals have four genuinely allocating positive controls.
These checks are not an absolute allocator census or an execution of every
function in a corpus.

The uninterrupted post-repair host gate on isolated validation revision
`8ca6fc5a` passes **5669 tests, zero failures and eight normal ignores**, with both
actual zero-test doctest packages completed. All 27 required optional bodies
execute, including real proxy, dataframe and linalg cases. Real HTTP fuzzing
requires at least 20000 completed iterations; headless Neovim assertions execute.

That host revision contains only the separately reviewed Rust proxy-fixture
patch from local PR #918 over the checked-default source. Production compiler,
runtime, std, Cargo and native Plenty fixtures remain unchanged from `830763c9`.
This is not a direct gate of either untouched PR branch or all current main.
Ten authenticated same-ID container removals and subsequent absences are proved;
missing final wrapper classifications remain unknown. Older external engine
builds establish compatibility, not fresh-engine qualification. Ten non-test
example paths' pre-clean absences remain unobserved; critical build freshness is
separately verified.

The earlier uninterrupted gate remains failed: 5658 passed, one Caddy readiness
failure and eight ignores. Two fixtures selected the same port. Later success
does not relabel that historical result.

## Repository benchmark template

The compiler benchmark generator adopts precise independent `Result`, `Ok` and
callee `?` contracts, with no per-operator wrapping. Original options, defaults,
1000/4000 scaling, declaration/operation order, warmups, timing boundaries, cache
policy and all five existing tests are unchanged. All 1000 generated unit bodies
match the frozen I corpus; only main replaces its old `Failure` with the precise
I/O, allocation, overflow and zero-division union. Frozen inputs are unchanged.

Four new tests bring example coverage to nine passing tests. Both complete
1000/4000 sources type-check, freshly compile and execute exact original
checksums. Failure cases preserve earlier field mutation and require independent
callee/main error declarations. Fifteen arithmetic integration tests, formatting
and strict workspace/all-target Clippy pass. A separate target reuses inactive
dependencies only after cleaning both compiler/runtime packages and checking
critical artifact absence. These are functional tests, not new performance data.

Final helper CI initially reports 333 tests with 60 errors and three normal skips:
the historical-template hash guard correctly rejects the newly adopted live
example. Issue #732 separates the inputs: an exact full historical Rust fixture
retains SHA `14de3ba2`, authenticated in one byte read before extraction. Current
example changes cannot affect it; missing/tampered fixtures fail closed without a
fallback. All 62 regenerated source files and 142 complete case expectations
remain byte/value-identical to the frozen native inputs. The failed CI log and
old helper remain preserved; focused 84 acceptance and 102 profile tests pass.
No comparison compiler, frozen input, measured result or receipt is refreshed.

## Controlled general collection

One serial, interleaved schedule retains **2466 timed invocations**:

- Runtime: 270 warmups and 1620 samples; five warmups and 30 samples per group.
- Compiler: 96 warmups and 480 samples; three warmups and 15 samples per group.
- Total: 366 warmups and 2100 post-warmup samples; 86 complete groups and 75
  available paired-ratio distributions.

All slots pass their original oracles. The 350 timed exit-1 arithmetic traps
are required outcomes, not waived failures. Recovered repeated errors remain
separate outcomes. No sample, warmup, failure or outlier was discarded. Duplicate
final command-log metarecords are not additional executions. All four general
phases pass; the helper's `packet_complete=false` and `acceptance_complete=false`
remain unchanged. Collection success is not whole-proposal acceptance.

Selected paired wall ratios are median **[p10, p90]**, not confidence intervals:

| Runtime success case | U/T | I/C |
|---|---|---|
| Tight loop | 0.986 [0.880, 1.076] | 0.982 [0.906, 1.052] |
| Calls | 0.993 [0.877, 1.141] | 1.003 [0.886, 1.144] |
| Text full output | 0.978 [0.903, 1.065] | 0.982 [0.926, 1.072] |
| Text success | 0.964 [0.948, 0.986] | 0.982 [0.964, 1.004] |

| Compiler action | 1000 units U/T; I/C | 4000 units U/T; I/C |
|---|---|---|
| Check | 1.002; 0.985 | 1.012; 0.976 |
| Debug object | 1.009; 0.981 | 1.009; 0.987 |
| Debug AOT | 1.002; 0.986 | 1.000; 0.990 |
| Release AOT | 1.015; 0.990 | 1.008; 0.989 |

Complete distributions, including all error paths and available C/T and I/U
pairs, remain in the local review evidence. Within-group `(p90-p10)/median`
ranges from 2.43–55.13% for runtime and 1.12–14.77% for compiler groups. These
whole-process observations do not establish a general speedup: startup, parsing,
printing and teardown remain included. Compiler wall time includes the GNU-time
wrapper; its maximum observed 327952 KiB RSS covers the compiler and descendants,
not the whole machine or Rust compiler-build cost.

## Code and allocation evidence

All 22 code reports pass, retaining 44 debug-object/optimized-linked-ELF
structures and 110 nonempty compiler dumps. Corpus IR excerpts select
`work999`/`work3999`; this is not a function-by-function audit of all generated
functions. Linked ELF includes runtime/startup effects and is not an optimized
application-object measurement.

Read-only optimized-IR/native analysis inspects 20 actual linked symbols: kernel,
caller, text consumer and each corpus's designated work function in all four
modes. Counts below cover only each named symbol, excluding callees:

| Target | Overflow guards T/U; C/I | Result-tag tests T/U; C; I | Release sites T/U; C; I |
|---|---|---|---|
| Tight kernel | 1; 1 | 0; 0; 0 | 0; 0; 0 |
| Caller | 2; 2 | 0; 1; 1 | 0; 0; 0 |
| Text consumer | 1; 1 | 1; 1; 1 | 1; 2; 2 |
| Each `work999` / `work3999` | 6; 2 | 1; 5; 6 | 2; 5; 4 |

The first three targets match within each behavior-preserving pair. Selected
successful loop passes contain nine instructions in every tight kernel, and
21 in T/U versus 24 in C/I callers, excluding the retained leaf body. These are
static path counts, not measured instruction totals or a cross-contract speed
comparison. Arithmetic uses plain values and shared failure exits, not successful
per-operator Result objects. Text's second C/I release site is failure cleanup,
not a second release on success.

Both corpus sizes independently show one additional C→I Result widening/tag test
and one fewer release site: I sends normal list cleanup through the common exit.
Stack reservation changes from `0x120` to `0x110`. The six-versus-two local guard
count does not mean lost protection: T/U inline squared-length arithmetic;
C/I retain that independently fallible call. Earlier field updates and ordered
cleanup remain before later failure handling.

Selected native returns use RAX alone for T/U kernel/caller values, and RAX/RCX
for recoverable kernel/caller and every text/corpus result. Optimized CLIF agrees;
no behavior-preserving pair changes its return carrier or introduces visible
hidden return storage. This does not prove all functions or ABIs equivalent.

One official full-profile invocation passes all **16** separate compiler jobs:
T/C/I/U × 1000/4000 × count/bytes, normal `--emit-object`. Every report retains
39 allocation and 39 timing rows, including all five required minimum phases
and role-specific source locations.

| Mode | Units | Exact attributed requests | Approximate attributed bytes |
|---|---:|---:|---:|
| T | 1000 | 3341485 | 706.2 MB |
| T | 4000 | 13373869 | 2.7 GB |
| C | 1000 | 4388551 | 894.7 MB |
| C | 4000 | 17528900 | 3.5 GB |
| I | 1000 | 4349233 | 885.6 MB |
| I | 4000 | 17382219 | 3.4 GB |
| U | 1000 | 3349154 | 709.0 MB |
| U | 4000 | 13414450 | 2.8 GB |

Counts are exact attributed successful allocator requests, including realloc
new-size requests above mimalloc. Bytes are formatted **APPROXIMATE** values
using binary 1024 units, not recovered exact totals. Count and byte values come
from separate processes: do not divide them to infer an observed average size.
Exclusive attribution excludes nested measured guards on the same thread but
includes unmeasured callees; worker attribution is not a complete process census.
Calls mean phase invocations. Inclusive, potentially parallel profiling timings
must not be summed as latency or compared with uninstrumented timings.

All eight approved C→I/T→U comparisons retain 312 common-phase entries: 118 exact
count fractions, 118 approximate byte ratios and 76 unavailable zero-denominator
ratios. Added/removed phase maps are retained; no missing phase was dropped.
No retained-heap, generated-runtime allocation or instrumentation-equivalence
claim follows.

The first two-job profiling attempt remains blocked before either producer ran
because of issue #731's system-tool hard-link guard. The reviewed correction
admits only authenticated native-tool context, binds fresh link-count/ctime and
consumes the exact authenticated record bytes. Source, proof, helper and output
identities keep strict nonalias rules. A new independently reviewed driver,
58 synthetic regressions, a successful two-job smoke and the later full16 run
are separate evidence; the original failure remains unchanged.

## Observation limits

Collection was serial during a user-reserved window, without other managed
builds, tests or writers. Fresh pre/post checks retained ordinary desktop activity
without noise subtraction. Endpoint aggregate idle exceeded 98% for general
collection and 97% for profiling; endpoints do not prove continuous inactivity.
No cache flushing, affinity/governor changes or foreign-process signals were used.
All raw reports, streams, artifacts, warmups and samples remain retained.

The bounded native/export audit preserves typed caller-owned error storage,
Unit entry-stack validation, ordered cleanup/draining and earlier successful
mutation. Executor-reduction opcode 9's seven initialized operands are distinct
from the separate collection dispatcher's five-operand limit. Existing C adapters
retain explicit status/output marshalling and reject unsupported error layouts;
no general cross-language native `Result` ABI is invented.

This is x86-64 host evidence, not universal cross-ISA, sanitizer, all-FFI or
all-Rust-arithmetic coverage. Independent archive-decimal and PNG-framing defects
remain issues #705 and #707, outside this arithmetic adoption.
