# Bigint development comparison

This experiment compares the new direct Plenty `std.bigint` implementation
with **Malachite 0.14.0**, independently of Plenty's runtime dependencies. The
separate Cargo workspace/lockfile is development tooling. Malachite code is
neither copied into the Plenty module nor linked into Plenty executables or
release artifacts. Malachite's `malachite-nz` dependency is LGPL-3.0-only.

The implementation commit measured is `b7780d34`, based on `main` at
`2756a8d0`. This experiment adds no production optimization or performance
promise. The measured gaps are tracked in local **issue #987**.

## Correctness

`compare.py generate` creates shared, deterministic hexadecimal operands
(seed 481983), a TSV corpus, and a Plenty driver from `oracle.plenty.in`.
The Rust oracle reads that exact TSV. **195 cases and 16,378 output lines
matched**, including exact IEEE f64 bit patterns after normalizing decimal
spelling. [The summary](results/differential-summary.json) records the corpus
hash. Generated inputs and complete raw outputs remain under
`target/bigint-comparison/` in the task worktree and can be reproduced.

Cases include zero, signed zero text, explicit plus signs, leading zeroes,
uppercase digits, both signs around i64/u64 boundaries, 31/32/33/64/128-limb
inputs, all-one and pseudorandom limbs, highly unbalanced operands, and twelve
constructed normalized base-2³² division estimates exceeding their exact
quotient digit. Each division-estimate case also runs with opposite signs.
Guard/sticky and ties-to-even inputs include the finite/infinite f64 boundary.
Each case checks arithmetic, floor quotient/remainder, signed bitwise
operations, shifts, exact i64/u64 conversion, and rendering plus reparsing in
every radix from 2 through 36.

Semantic choices are explicit:

- Use Malachite's `DivMod::div_mod`, which floors and gives the remainder the
  divisor's sign. Rust/Malachite `/` and `%` would truncate and give incorrect
  comparisons for opposite signs.
- Use `RoundingFrom<&Integer>` with `Nearest`, including ties to even. Its
  `TryFrom` float conversion instead requires an exact representation.
- Unsigned shift counts match Plenty's accepted nonnegative domain. Negative
  **operands** exercise floor right shifts. Malachite additionally treats
  negative signed shift counts as the opposite direction; Plenty traps those
  counts, verified separately by `test_std_bigint`.
- Malachite supports radices through 62, while Plenty deliberately supports
  2–36. The comparison covers that entire common range. Plenty's invalid
  input, invalid radix, digit-limit and zero-division errors, allocation
  injection, and allocation-free inline arithmetic are checked by the focused
  integration tests. Malachite has no matching fallible allocation contract.

## Measurements

Read the [complete median table](results/summary.md),
[raw elapsed batches](results/wall.csv), [raw instruction counts](results/instructions.csv),
and [host/build metadata](results/metadata.json). Measurements use an AMD
Ryzen 7 7840HS, Linux x86-64, rustc 1.100.0/LLVM 23, and CPU affinity 0.
Rust uses `--release`, opt-level 3, one codegen unit, panic abort, default
target and Malachite features. Plenty uses a release compiler and its default
optimized program mode, without `--fast-compile` or runtime-checks.
No other heavy repository job ran during the measurement window.

Five alternating elapsed-time batches per engine run each operation repeatedly
on the same pre-generated operands. The operation dispatch and input setup are
outside the timed loop. Parsing/rendering inputs, process startup and output IO
are outside the internal monotonic timer. Counts are calibrated against Plenty
and shared by both engines; every batch has a matching checksum. Rust uses
`black_box` to stop invariant operands/results being optimized away. All
operations borrow operands and produce fresh results; allocation, deallocation,
fallible result handling, helper calls and checksum work remain in the timed
body. These are API workloads, not pure instruction latency measurements.
String checksum length scans may differ between Plenty text and Rust bytes;
both outputs contain only ASCII digits. Small-operation results especially
include call, representation and loop overhead.

Three instruction-counter pairs run N and 2N iterations. Subtracting their
`instructions:u` counts removes fixed startup, operand parsing, setup rendering
and output cost. The CSV retains both raw counts and their per-operation
difference. The largest within-shape relative counter spread was 0.46%
(256-bit Malachite rendering). Internal timing and counters are separate runs;
elapsed time is sensitive to clocks, shared host activity and temperature and
is supplementary evidence, not a cross-machine throughput guarantee.

For division, the displayed size is the divisor width: its numerator is the
pre-generated product of two numbers of that width. Other operations use two
numbers of the displayed width. Parse/render use decimal; the Plenty cap is
100,000 digits, above every input. The largest size is 65,536 bits.

The large instruction gap remains across arithmetic and conversion. Malachite
uses different algorithms and representations and can use specialized native
kernels; Plenty currently uses fallible list storage, wrapper/result plumbing,
schoolbook/Karatsuba multiplication, base-2³² Knuth division, and recursive
radix conversion. This experiment does not apportion every whole-operation
instruction to a cause or establish a native-kernel crossover. It measures no
ARM64 behavior, peak memory, allocation counts, concurrency or cryptographic
timing properties.

## Isolated limb evidence

`limb.plenty` and `limb.c` isolate an allocation-free, single-word multiplier
with the same view lengths, output bounds and carry arithmetic. C has no
`restrict` qualifier and explicitly checks the output bound and high-word
addition. Both executables print the [same result](results/limb-output.txt).
The [Plenty listing](results/limb-plenty-loops.txt) and
[C listing](results/limb-c-loops.txt) were counted with the existing
`experiments/kernels/loops.py`; gcc is 16.2.1 at `-O2`.

| Static loop instruction group | Plenty | C | Attribution |
| --- | ---: | ---: | --- |
| Arithmetic, data load/store and counted-loop work | 10 | 8 | C folds an input load into `mul` and reuses carry flags |
| Tuple slots, padding zeroes and boolean widening | 17 | 1 | Low/high and sum/carry round-trip through inline tuple slots; C only zeroes its carry destination |
| Operand/register moves | 3 | 1 | Extra index and loop-carried register moves |
| Output bounds check | 2 | 2 | Both preserve the checked output contract |
| Duplicate low-half multiplication | 2 | 0 | Plenty has a low `imul` plus unsigned `mul`, whose low result is unused |
| Carry selection | 5 | 0 | Both arms of Plenty's boolean-to-word diamond; C uses its boolean directly |
| High-word overflow check | 1 | 0 | C proves the multiply/add relation prevents overflow |
| Extra loop jump | 1 | 0 | Plenty retains exit test plus backward jump |
| **Static union** | **41** | **12** | |

The script counts the union of the carry diamond's blocks, not a single
dynamic path: Plenty executes 39 or 40 of these instructions per iteration;
C executes 12. The existing full bigint schoolbook inner loop's static union
is [87 instructions](results/multiply-loops.txt), including more tuple traffic,
reloaded output metadata and checked `i+j` accesses. It also has carry diamonds,
so 87 is not an exact dynamic per-product count.

No checks were removed, compiler/backend changes attempted, or IR variants
benchmarked. The attribution identifies follow-up questions; it does not claim
that scalarizing tuples alone closes the whole-operation gap. Compiler-build
instruction counts and before/after optimization tables therefore do not apply.

## Reproduce

From the repository root, with localhost/cache access needed by ordinary builds
and `perf` instruction-counter access for the benchmark:

```sh
RUSTC_WRAPPER= cargo build --locked --release --bin plenty
RUSTC_WRAPPER= cargo build --locked --release \
  --manifest-path experiments/bigint-comparison/Cargo.toml \
  --target-dir target/bigint-comparison/rust-target
python3 experiments/bigint-comparison/compare.py generate
target/release/plenty --compile target/bigint-comparison/oracle.plenty \
  -o target/bigint-comparison/oracle
target/release/plenty --compile experiments/bigint-comparison/bench.plenty \
  -o target/bigint-comparison/bench
python3 experiments/bigint-comparison/compare.py compare \
  --malachite target/bigint-comparison/rust-target/release/plenty-bigint-comparison
python3 experiments/bigint-comparison/compare.py benchmark \
  --malachite target/bigint-comparison/rust-target/release/plenty-bigint-comparison

target/release/plenty --compile experiments/bigint-comparison/limb.plenty \
  -o target/bigint-comparison/limb
gcc -O2 -fno-tree-loop-distribute-patterns experiments/bigint-comparison/limb.c \
  -o target/bigint-comparison/limb-c
target/release/plenty --dump asm --dump-filter limb \
  experiments/bigint-comparison/limb.plenty -o target/bigint-comparison/limb-asm
objdump -d --no-show-raw-insn target/bigint-comparison/limb-c \
  > target/bigint-comparison/limb-c.asm
python3 experiments/kernels/loops.py -v < target/bigint-comparison/limb-c.asm
```

For Plenty dumps, lowercase the hexadecimal instruction-address field before
passing it to `loops.py` (the dump may use uppercase digits). The measured
compiler was built with the primary checkout's `CARGO_TARGET_DIR`; substitute
that directory's release executable when sharing build artifacts. The generated
target directory retains corpus, full oracle output, perf stderr/stdout for each
pair, and assembly; `results/` retains the reviewable summaries and all raw batch
counts. Reruns regenerate inputs deterministically but do not overwrite the
committed evidence snapshots.

## Sources

- [Malachite Integer 0.14.0](https://docs.rs/malachite-nz/0.14.0/malachite_nz/integer/struct.Integer.html): independent development oracle and comparator, LGPL-3.0-only; current documentation and cached crate source were consulted for the APIs and semantics above.
- [Emitted-code procedure](../../book/src/development/emitted-code.md) and [existing kernel tools](../kernels/README.md): optimized-build, counter and assembly procedure. The kernels here are original small reproductions.
