# Selected-field idiom measurements

The [columnar-list tutorial](../../tutorial/special-purpose/columnar-lists/index.md)
compares ordinary list and clist source for one-field aggregation, conditional
two-field aggregation and updating positions from velocities (issue #887).
The experiment under `experiments/columnar-list/idioms/` extracts those exact
functions from the tutorial and checks eight- and 64-field schemas.

## Emitted work

Optimized Plenty 0.8.19 at compiler source commit `365a3b3b`, x86-64, compared
with gcc 16.2.1 scalar C kernels. Counts cover a standalone inner-loop pass,
including the successful condition for conditional aggregation:

| Task | C AoS | Plenty list | C SoA | Plenty clist |
| --- | ---: | ---: | ---: | ---: |
| Aggregate mass | 4 | 6 | 4 | 5 |
| Conditional mass | 7 | 9 | 7 | 8 |
| Update positions | 11 | 15 | 11 | 14 |

The extra list aggregate instructions compute a row stride and load separately
from addition. Clist aggregation adds only the separate load. Conditional list
aggregation adds row-stride/address computation, while the clist loop adds a
countdown update. Updating positions adds two temporary-register copies of
`dt` to either representation, plus list row-address computation or the clist
countdown. This accounts for the differences in the table. C loop unrolling,
vectorization and fast-math are not enabled; the C struct is native storage,
not a reproduction of Plenty's tagged list layout.

The clist loops read only the required columns, without row reconstruction,
generic field-access calls or per-row owner reloads. Code after inlining can
have different register allocation; inspect the measured caller as well as
the standalone function. `instructions.txt` retains both sets of counts.
No compiler change or before/after performance improvement is claimed.

## Zipped columns

The supported `column(...).zip(column(...))` forms retain generic iterator
state. Both a zipped loop and zipped fold have 43 instructions on the standalone
matching path, compared with eight for the indexed conditional aggregation.
The IR retains two source counters and optional-item state; emitted code
constructs/tests presence flags and moves floats through integer registers.
Issue #888 records this gap. The tutorial recommends indexed loops for
multi-field passes today; the experiment retains `zip-probe.plenty` so a
future change can compare all three forms.

## Validation and timing limits

The runner checks both layouts against independent Python checksum calculations
for 1, 7, 8, 17, 129, 8,192 and 262,144 rows. The wide schema adds 56 unused
`f64` fields to the tutorial's eight fields. The retained `validation.json`
records checksums and source/compiler hashes. The tutorial's complete example
is also checked against its expected output.

Instruction counts are not elapsed-time speedups and do not measure cache
locality. Concurrent builds prevented an uncontended timing window, so no
wall-clock comparison is published. The source includes a CPU-pinned timing
runner with alternating layout order, warmups, independent checksums and an
optional build/test interference monitor. It excludes construction from scan
timings; whole-process retired-instruction counters include construction and
both layouts. Tiny, whole-row, growth and deletion workloads are outside this
comparison. The experiment README gives the full commands and conditions.
