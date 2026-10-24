# Direct indexed column access

Issue #432 identified generic runtime dispatch as the dominant cost of indexed
`clist` fields and `copy_row`. The [initial implementation](native-implementation.md)
already emitted direct loads for column pipelines, but indexed eight-field work
made eight dispatcher calls per row. Row copying walked the schema, recalculated
column prefixes, and copied the resulting tagged row again.

## Implementation and constraints

The compiler now calculates column prefixes from the checked primitive schema.
Reads and writes use native-width memory operations. A row copy checks its index
once, loads the fields, and writes directly into fresh ordinary row storage.
The existing column-pipeline setup is unchanged. The compiler and runtime share
layout offsets, checked against the runtime's `repr(C)` header.

Null owners are checked before reading the header. Negative indices are
normalized without signed overflow, and an unsigned comparison rejects both
still-negative and too-large indices. Indexed access retains its source-located
trap through a small cold helper; `copy_row` returns `Nothing` for missing rows.
Field types determine the actual load/store width and row-slot encoding, including
signed narrow integers, unsigned maxima and floating-point bit patterns.

There is no new frontend analysis or pointer cache across source operations.
Each operation reads the current owner, so reserve, append and replacement
between operations cannot leave stale metadata. Existing loans, evaluation order
and checked arithmetic are unchanged. General loop hoisting, automatic SIMD and
parallel execution are outside this change.

## Instruction-count evidence

`experiments/columnar-list/plenty/count_access.py` runs the same indexed and
sequential-copy kernels as `bench.py`, one at a time, for 200 passes over 8,192
rows. Linux `perf stat` counts user-mode instructions and cycles in three
alternating before/after samples, pinned to CPU 8. Every checksum is checked.
Compilation finishes before counting. This is useful under CPU contention;
cycles and elapsed time still require a separate quiet timing comparison.

Counts include startup, construction, output and teardown. A zero-pass control
reports their cost separately; the table does not subtract it or claim isolated
kernel counts.

`experiments/columnar-list/plenty/indexed-instructions.json` retains the samples,
compiler/source/binary hashes and counter scope. The baseline is local commit
`697aedb5626d74b7f97f1a51bcd3334823df5744`.

| Workload | Before instructions | After instructions | Reduction |
| --- | ---: | ---: | ---: |
| Construction-only control | 15,870,296 | 15,870,567 | Essentially unchanged |
| Eight indexed fields | 3,043,642,962 | 255,083,611 | 11.9× fewer |
| Sequential row copies | 2,391,559,687 | 240,339,982 | 10.0× fewer |

The counts varied by fewer than 400 instructions across the three samples for
each case. They support the removed-work explanation, rather than substituting
for elapsed-time verification.

Reproduce with two compilers built without allocation instrumentation:

```sh
python3 -B experiments/columnar-list/plenty/count_access.py \
  /path/to/before/plenty /path/to/after/plenty --out /tmp/clist-counts \
  --rows 8192 --repeats 200 --samples 3 --cpu 8
```

The reference and tutorial continue to recommend column pipelines for selected
field scans. Whole-row reconstruction still gathers separate fields and moves
ordinary tagged row storage; removing dispatch does not make this free or prove
that `clist` is universally faster than an ordinary row list.

## Final timing verification

`experiments/columnar-list/plenty/native-indexed-measurements.json` retains final samples,
compiler/source/executable hashes, kernel sizes, frequency readings and rejected
attempts. These use the same AMD Ryzen 7 7840HS, pinned to CPU 8. Both compilers
were built without allocation instrumentation. The runner waited for competing
builds/tests to finish, checked host processes every 250 ms during execution,
and rejected one large after-run when a build began. Earlier interfered timings
are not mixed into these results. This excludes detected build interference,
not all operating-system noise. Median sampled frequency was about 4.092 GHz;
the artifact retains occasional lower readings as well.

The existing `bench.py` program checks every checksum, warms up once, alternates
the ordinary/native layout order, and measures seven samples. Large cases use
20 repetitions; tiny cases use 100,000. All executables were prepared before
the final timing pass. Median nanoseconds per row:

| Rows | Indexed fields before | Indexed fields after | Row copy before | Row copy after |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 119.778 | 7.496 | 97.596 | 14.830 |
| 8 | 105.703 | 6.477 | 80.813 | 12.457 |
| 64 | 99.223 | 6.519 | 80.060 | 12.780 |
| 8,192 | 115.951 | 6.419 | 94.516 | 12.331 |
| 262,144 | 117.983 | 6.526 | 96.029 | 12.558 |

At 262,144 rows, indexing improved about **18.1×** and sequential copying about
**7.6×**. The after-run ordinary-row controls were 3.985 and 4.031 ns/row:
indexed clist work is still about 1.6× slower and row copying about 3.1× slower.
The copy kernel still moves the unwrapped ordinary row into local storage with
a 128-byte `memmove`. Removing schema dispatch does not remove that cost.

The large indexed function shrank from 1,269 to 991 bytes; the copy function
grew from 831 to 961 bytes because its field loads are now explicit. These sizes
exclude shared helpers. Neither valid indexed access nor reconstruction calls
the generic collection dispatcher. Cold range-validation code elsewhere in the
benchmark remains separate from the per-row access path.

**The one-column control is a limitation:** its large median changed from
0.497 to 0.743 ns/row, despite identical emitted kernel assembly and unchanged
294-byte function size. The 8,192-row case similarly changed from 0.494 to
0.736 ns/row. Code placement/alignment is a candidate explanation, not an
isolated causal result. An exploratory change to inline column setup was
removed; the final patch retains the original pipeline setup and loop. This
observation is why instruction counts alone do not justify a universal speedup
claim. The target indexed/copy improvements remain clear in both counters and
elapsed time. Issue #437 records the scan-throughput follow-up separately.

### Bounded placement check (issue #437)

The follow-up found reproducible address sensitivity, so the observation cannot
be dismissed as timing noise alone. `probe_scan_layout.py` compiles the one-field
workload once and relinks that **same object** with GNU ld at eight text-section
addresses. Linker alignment produces four distinct function placements. Each
binary performs 20,000 scans of 8,192 rows; checksums are checked, order is shuffled
with seed 437, and one warmup precedes three measured samples per placement.
Two independent runs produced the same object hash and 294-byte scan function.

| Function address modulo 64 | Cycles/row across the two runs | First-run time, ns/row |
| ---: | ---: | ---: |
| 9 | 2.045–2.066 | 0.500–0.502 |
| 25 | 2.048–2.065 | 0.505–0.507 |
| 41 | 3.048–3.070 | 0.747 |
| 57 | 4.055–4.084 | 0.996–0.997 |

Retired instruction counts remain essentially identical. Hardware counters cover
the whole process; internal elapsed timing covers the repeated folds. The second
run has noisier elapsed times, while the cycle groups remain distinct. Modulo 64
is an address label here, not a claim about the CPU's exact fetch-block size.
Instruction-fetch/decode alignment is a plausible mechanism, but the probe moves
the whole text section and does not isolate a specific CPU front-end effect or
prove the precise cause in the original two binaries.

The probe reused an available compiler executable instead of rebuilding Rust;
its hash, the object hash and all samples are retained in
`experiments/columnar-list/plenty/scan-layout-measurements.json`. Reproduce on
x86-64 Linux with GNU ld, `nm`, and access to user-mode `perf` counters:

```sh
python3 -B experiments/columnar-list/plenty/probe_scan_layout.py \
  /path/to/plenty --out /tmp/clist-scan-placement
```

This completes the bounded investigation. **No compiler change is proposed.**
A universal padding/alignment policy would need broader benchmarks and code-size
tradeoffs; issue #437 remains a low-priority performance concern. The placement
experiment is evidence, not a hard-coded alignment recommendation.

The subsequent [function-alignment evaluation](alignment-policy.md) tests
16-, 32- and 64-byte boundaries against short scans, a two-column fold and an
unrelated scalar loop. It rejects a blanket alignment change: some placements
improve while others regress.

Reproduce the timing programs on each compiler, without competing builds:

```sh
python3 -B experiments/columnar-list/plenty/bench.py /path/to/plenty \
  --native-clist --skip-generated --out /tmp/clist-access-large \
  --rows 8192 262144 --repeats 20 --samples 7 --cpu 8
python3 -B experiments/columnar-list/plenty/bench.py /path/to/plenty \
  --native-clist --skip-generated --out /tmp/clist-access-small \
  --rows 1 8 64 --repeats 100000 --samples 7 --cpu 8
```

The full all-features workspace suite, Clippy, formatting, documentation links
and book build pass. Focused cases cover development/optimized modes, mixed
primitive widths, growth, copies returned across calls, independent copies,
extreme indices, source-located traps, and absence of generic reconstruction
dispatch. Existing tests retain evaluation-order, enum-projection, allocation
failure and owner-loan coverage.
