# Selected-field idiom measurements

The [columnar-list tutorial](../../tutorial/special-purpose/columnar-lists/index.md)
compares ordinary list and clist source for one-field aggregation, conditional
two-field aggregation and updating positions from velocities (issue #887).
The experiment under `experiments/columnar-list/idioms/` extracts those exact
functions from the tutorial and checks eight- and 64-field schemas.

## Emitted work

Optimized Plenty 0.8.19 at compiler source commit `c63d0884`, x86-64, compared
with gcc 16.2.1 scalar C kernels. Counts cover a standalone inner-loop pass,
including the successful condition for conditional aggregation:

| Task | C AoS | Plenty list | C SoA | Plenty clist |
| --- | ---: | ---: | ---: | ---: |
| Aggregate mass | 4 | 6 | 4 | 5 |
| Conditional mass | 7 | 9 | 7 | 8 |
| Update positions | 11 | 15 | 11 | 14 |

The extra list aggregate instructions compute a row stride and load separately
from addition. Clist aggregation adds only the separate load. Conditional list
aggregation adds row-stride/address computation. The zipped clist query loads
mass separately before the condition; C combines its mass load and addition on
the matching path. Updating positions adds two temporary-register copies of
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

Read-only multi-field examples zip selected columns. After the shared-cursor
and immutable-capture fixes (issues #888 and #891), the explicit zipped loop,
conditional captured fold and indexed alternative all have eight instructions
on the standalone matching path in `zip-probe.plenty`. The exact tutorial
query uses the explicit loop; its column plans have the same owner and row order.
Indexed field writes remain the recommended pattern for updates.

The original experiment at `365a3b3b` counted 43 instructions for each zipped
form versus eight for indexing. That is historical evidence of the defect,
not a limitation of the current compiler. The retained probe is rechecked
with the current compiler; see the [shared-cursor evidence](../../completed-work/performance.md#selected-field-column-zip-issue-888)
and [conditional capture evidence](../../completed-work/performance.md#conditional-callback-captures-issue-891)
for the compiler changes and their independent measurements. The latter's
7.42% whole-program instruction reduction is not a list-versus-clist speedup.

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
