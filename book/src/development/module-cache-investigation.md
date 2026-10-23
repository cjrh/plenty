# Per-module cache investigation

The 2026-10-24 bounded B19/B17 investigation supports keeping the current
function/program caches. A scalar-only separate-compilation spike proved reuse
and invalidation at an explicit boundary, but was slower than compiling the
same computation normally. It is not a production cache implementation.

Frontend reuse remains promising for imports with substantial bodies, especially
the standard library. It requires a real lowering boundary; independently
packaging today's modules as C libraries is not a useful shortcut. This report
records evidence and constraints. Scheduling remains in the [backlog](../backlog.md).

## Current compiler measurements

Local `main` at `00d32a13`, AMD Ryzen 7 7840HS, Linux x86-64, Rust 1.99.0,
release compiler with `profile` compiled in but inactive during timings.
Compiler SHA-256:
`61875f0816906d4c5d2577da75bb7f918ad9a26115029bc772d188176d6afc07`.
Seven repetitions, physical cores `0,2,4,6,8,10,12,14`, other agent builds paused.
The existing [module-edit harness](module-edits.md) generates ten modules with
100 independent functions each, a caller and an entry point. Its broad-import
variant imports `std.http.server`; the scalar control omits that import.

| Object build | Broad import, median ms | Scalar control, median ms | Broad import peak RSS, KiB |
|---|---:|---:|---:|
| Cache disabled | 45.50 | 17.14 | 35,348 |
| Populate empty cache | 47.73 | 17.66 | 36,264 |
| Warm functions | 44.54 | 15.56 | 33,788 |
| Unchanged program | 3.99 | 4.06 | 8,096 |
| Ordinary body edit | 44.00 | 16.17 | 34,044 |
| Interface and caller edit | 46.79 | 16.35 | 35,804 |
| Generic body edit | 44.67 | 16.66 | 35,588 |

These timings include compiler startup and object writing, but exclude native
linking. Every cached artifact matched its uncached counterpart byte for byte,
and every source variant executed with its expected output. No-change builds
hit the program cache; all three edits missed it. This is a synthetic workload,
not a forecast for arbitrary applications.

Separate timing profiles after a body edit recorded:

| Phase | Broad import | Scalar control |
|---|---:|---:|
| Parse all modules | 8.35 ms / 25 modules | 2.20 ms / 12 modules |
| Resolve names | 3.32 ms | 1.16 ms |
| Lower program | 19.28 ms | 2.66 ms |
| Lower functions, inside program lowering | 10.91 ms / 1,629 functions | 1.42 ms / 1,004 functions |
| Independent operation check | 2.21 ms | 0.49 ms |

Nested profile rows are not additive. The broad-import frontend contains much
more reusable work than the scalar control. Avoiding all this work is only an
optimistic ceiling: loading, dependency validation, merging and changed-module
checking still cost time.

A separate `profile-alloc` build reported 119.8 MB allocated with caching
disabled and 74.5 MB after a body edit (hotpath's displayed MB units). Parsing
allocated 10.4 MB, name resolution 4.8 MB and function-body lowering 19.6 MB
after that edit. These are exclusive allocation traffic counters, not retained
memory or RSS. Allocation-instrumented latency is excluded from all timing
tables. This leaves meaningful allocation work for a frontend cache to avoid,
but artifact decoding would introduce allocations of its own.

Five additional broad-import `--release` repetitions measured optimized
**executable** builds, including linking: 79.77 ms uncached, 33.77 ms unchanged,
73.49 ms body edit, 71.78 ms interface edit and 77.73 ms generic edit. Cached
executables again matched uncached executables. Reusing frontend artifacts alone
does not eliminate the link floor.

## Actual reusable-artifact spike

`experiments/module-cache/run.py` generates ten scalar modules, each with a
single exported `i64` function and 100 independent private functions. A separate
caller uses the first module. It caches existing static-library outputs using
compiler-binary, source, module-name and imported-interface hashes. Integrity
checks reject damaged artifacts. A second mode extracts only each archive's
`plenty_exports.o`, leaving the final application to supply the runtime.

This intentionally uses existing C export/import support rather than modifying
the compiler. Interfaces are generated from the fixed fixture, not inferred by
a new parser. There are no generic imports, classes, borrowed values, strings,
closures or cross-module substitution at the artifact boundary. It proves that
an ordinary scalar body edit can reuse callers; it does not prove a general
Plenty interface is just its written signatures.

Five repetitions per mode, same compiler and CPU affinity. Each repetition has
an empty artifact cache. Existing Plenty caches are disabled to isolate artifact
reuse. The ordinary comparator builds the same scalar computation and independent
functions through normal imports, also with caching disabled. All rows below
include native linking and use default code optimization, unlike `--release`
above. Source generation and execution checks are outside the timings.

| Build | Full archive cache, ms | Extracted object cache, ms | Ordinary compilation, ms* |
|---|---:|---:|---:|
| Cold | 1,222.61 | 1,236.97 | 48.89 |
| Unchanged | 191.49 | 96.52 | — |
| Ordinary body edit | 279.73 | 197.80 | 55.36 |
| Interface and caller edit | 379.55 | 307.34 | 50.31 |

*Ordinary comparator from the extracted-object run. The independently repeated
archive-run comparator was 48.71 / 50.37 / 50.33 ms respectively. These modes
were run in separate batches; small differences are not an optimization claim.

An ordinary body edit rebuilt only module `m0`; its caller and the nine other
modules were hits. A signature edit rebuilt `m0` and the caller. Rebuilding all
artifacts afresh produced the same executable bytes as reuse for each edit.
The program produced 42, 43 and 44 in the base, body and interface cases.
Corrupting every cached artifact forced rebuilding and restored correct output.

Full archives occupied 174,083,980 bytes for the base program; extracted objects
occupied 335,712 bytes. The former repeats the library runtime for each module.
Hashing those archives on hits is part of the measurement. Extraction removes
that storage/read cost but still creates archives on misses. The extracted
mode's peak compiler-child RSS was about 55 MiB cold/edited and 32 MiB unchanged,
versus about 37 MiB for ordinary compilation. This excludes the Python driver's
RSS and is not a total allocation count. Object and archive modes have different
linker retention, so executable byte identity is checked within a mode, not
between modes or against ordinary compilation.

Repeated compiler processes, export adapters, archive construction and linking
outweigh avoided checking in this small workload. Do not adopt this mechanism
as the module cache. The experiment does not estimate the speed of a proper
in-process lowered-artifact cache, which avoids these costs.

## Required boundary and invalidation

The implementation still lowers a whole resolved program. `frontend::lower`
creates aliases, signatures, generic instances and pending work; lowering shares
the string heap and records information used by subsequent functions. The module
loader preserves dependency order, import resolution and source identities.
Persisting a function's operations while ignoring these outputs is insufficient.

The small `check_dependencies.py` probe changes only `&mut pair.x` to
`&mut pair.y` in an imported function. Its declaration stays
`select(pair: &mut Pair) -> &mut i64`, yet an unchanged caller that mutates
`pair.y` changes from accepted to a conflicting-borrow diagnostic. Reverting
the body restores acceptance. This directly exercises
`frontend::references::returned_fields`: inferred projections are interface facts.

| Dependency | What must invalidate or be reconstructed |
|---|---|
| Source/import resolution | Module text, canonical package/module identity, import target changes, compiler identity/settings |
| Public declarations | Signatures, nominal layouts, aliases, visibility, protocols and generated methods |
| Body-derived semantics | Returned-reference projections and any effects/capture facts consumed elsewhere |
| Generic instantiation | Defining body, type arguments, constraints and transitive instantiated dependencies |
| Native substitution | Any body embedded in another function, even if its signature is unchanged |
| Stored operations | Deterministic string/type/instance identities, source locations and ownership diagnostic origins |

A conservative first cache may depend on complete imported source digests;
this sacrifices reuse but avoids claiming interface precision prematurely.
Precision can follow after an explicit module result can be built and replayed
against fresh imported environments. Generic definitions/instances and embedded
native bodies need separate dependencies from ordinary declaration hashes.
Corrupt/version-mismatched artifacts must miss, and invalidated diagnostics must
remain in program order. Existing cache-on/off identity and import-resolution
tests remain acceptance gates.

The smallest credible compiler change is therefore to isolate and test a module
lowering result **without persistence first**. Measure replay against normal
lowering on an actual std-heavy application before adding serialization. The
earlier [syntax-tree/copy investigation](../design/toolchain/02-architecture/cache.md#later-phases)
already argues against inserting a parsed-tree cache instead. This investigation
does not authorize a broad redesign or promise a speedup.

## Reproduction

Build `plenty` with `cargo build --locked --release --features profile --bin plenty`.
Run `scripts/bench_module_edits.py` with `--runs 7 --profile`, then repeat with
`--import-std ''`; use `--runs 5 --release` for the optimized executable check.
Each command requires a `--json PATH`. Run the experiment with:

```sh
python3 experiments/module-cache/run.py target/release/plenty --runs 5 --json /tmp/archives.json
python3 experiments/module-cache/run.py target/release/plenty --runs 5 --objects --json /tmp/objects.json
python3 experiments/module-cache/check_dependencies.py target/release/plenty
```

Pin CPUs with `taskset` as above and avoid concurrent compilation. JSON records
every sample, compiler hash, affinity, outputs, reuse decisions and artifact
sizes. The existing module-edit harness additionally records separate phase
profiles and cache statistics. The spike is single-process orchestration in a
temporary directory; it is not a hardened shared cache or a general-purpose
module parser.

For allocation counts, rebuild with `--features profile-alloc` and run the
module-edit harness with `--runs 1 --profile`; keep those results separate from
timing runs. No production Rust code changed, so validation here is the executed
native experiments, edit matrix, Python syntax checks and Markdown link checks.
