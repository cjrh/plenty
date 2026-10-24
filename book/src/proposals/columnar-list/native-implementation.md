# Initial native column implementation

The [current interface](../../design/facilities/01-builtins/clists.md) implements
the bounded primitive-field version of `clist[T]`. The earlier
[generated-container experiment](implementation-spike.md) supplied the ownership
and performance baseline; its eight ordinary lists are not the native layout.

## Decisions at the representation boundary

One allocation holds a header and a contiguous buffer for each field. Native
widths are 1, 2, 4 or 8 bytes, including one-byte booleans. Rounding row capacity
to a multiple of eight aligns every column start. This avoids separate column
allocation failures: reserve prepares a complete replacement before publishing
it, and append advances the single row count only after all fields are written.
Failed append returns the unchanged input object in `AppendError[T]`.

Ordinary Plenty references point to tagged value slots, so they cannot safely
point into these packed buffers. A column therefore yields scalar **values**
through a local `Iter[FieldType]` while borrowing the entire owner. Its pointer
and length are loaded before the loop; iteration emits native scalar loads.
Existing fluent operations consume these values without new column-specific
map/filter/reduce methods. A live plan prevents owner mutation or movement.

`copy_row` materializes an ordinary scalar-field object without allocation.
There is no general `&T` row view, no packed field reference, and no mutable
column plan. Direct indexed field access is checked and uses runtime helpers.
Strings, nested classes, enums, custom destructors and fallible initializers
are rejected. These restrictions make ownership and failure cleanup explicit
without committing to a future reference representation.

## Reproducing the comparison

Build without allocation instrumentation, then run the extended original probe:

```sh
cargo build --locked --release --bin plenty
python3 -B experiments/columnar-list/plenty/bench.py target/release/plenty \
  --native-clist --out /tmp/plenty-native-columns \
  --rows 8192 262144 --repeats 20 --samples 7 --cpu 8
```

The runner checks every checksum, discards the first sample, alternates layout
order, and retains all owners during measurement. It compares native `clist`,
the original generated eight-list container, and ordinary `list[Particle]`.
Each row has eight `i64` fields. The kernels scan one field, filter using two
fields, read every field by index, materialize sequential complete rows, and
materialize rows in a fixed permutation. The historical `random` kernel uses
`(i * 65537) % rows`: this is a predictable permutation, and degenerates to
sequential order at 8192 rows. It is not an arbitrary random-access workload.
Construction is measured separately.

These are scalar x86-64 measurements. No automatic SIMD or parallel execution
is introduced, and no other architecture's performance is inferred. Contiguous
loads give later vectorization and partitioning work a useful representation;
they do not remove checked arithmetic, effects, aliasing or scheduling questions.

## Results and limits

`experiments/columnar-list/plenty/native-measurements.json` retains all samples,
compiler hash and host information. The run pinned CPU 8, stopped task builds,
and paused the other worker's compilation. The host nevertheless remained
frequency-limited: CPU 8 reported about 544 MHz with the performance governor.
Treat the following as same-run comparisons, not representative peak throughput
or a direct comparison with the earlier experiment's absolute timings.

Median nanoseconds per row, over seven measured samples:

| Rows | Operation | Ordinary row list | Generated columns | Native clist |
| ---: | --- | ---: | ---: | ---: |
| 8,192 | One-field fold | 11.95 | 7.09 | 4.49 |
| 8,192 | Two-field filtered fold | 22.09 | 25.02 | 22.62 |
| 8,192 | All fields by index | 23.50 | 58.71 | 980.24 |
| 8,192 | Sequential whole rows | 24.21 | 638.25 | 807.29 |
| 8,192 | Permuted whole rows | 59.72 | 641.95 | 794.65 |
| 262,144 | One-field fold | 36.61 | 6.48 | 4.16 |
| 262,144 | Two-field filtered fold | 100.44 | 25.97 | 23.14 |
| 262,144 | All fields by index | 29.28 | 69.48 | 927.71 |
| 262,144 | Sequential whole rows | 33.03 | 651.26 | 814.84 |
| 262,144 | Permuted whole rows | 196.67 | 1,194.90 | 921.35 |

The ordinary all-fields and sequential whole-row controls borrow rows directly;
the column kernels respectively index fields and materialize rows. They perform
the same calculation using the available interfaces, so this compares practical
access paths rather than isolating memory layout alone.

The large one- and two-field scans were about 8.8× and 4.3× faster than the row
list in this run. The small two-field case had no advantage. Native columns use
64 bytes of payload per eight-i64 row, compared with 128 tagged-slot bytes in
both baselines; one large native column occupies 2 MiB instead of 4 MiB.

Whole-row work is a substantial weakness of this first implementation:
sequential row copying was about 25× slower, and indexed eight-field work about
32× slower than the row-list controls. The native indexed kernel makes eight
generic dispatcher calls per row. Reconstruction walks the schema, recomputes
column prefixes and snapshots tagged row slots. These are current lowering
costs, not inherent costs of columnar storage. **Issue #432** records this
performance concern; it remains open after the initial implementation.

Reserved construction took about 1,271 / 4,209 / 1,220 ns per row for the large
ordinary / generated / native collections. These are single construction
samples, not a stable construction benchmark. They do not justify a general
append-speed claim.

## Generated code and correctness

`nm -S` on the large-case executable gives these kernel sizes in bytes, excluding
shared runtime helpers and measurement wrappers:

| Kernel | Ordinary row list | Generated columns | Native clist |
| --- | ---: | ---: | ---: |
| One-field fold | 161 | 160 | 294 |
| Two-field filtered fold | 235 | 408 | 620 |
| All fields by index | 362 | 1,856 | 1,269 |
| Sequential whole rows | 362 | 679 | 831 |
| Permuted whole rows | 560 | 792 | 936 |

The native one-field assembly has two setup helper calls, followed by a direct
eight-byte indexed load/add and the existing overflow check. There is no helper
call per element and no SIMD instruction in that loop. Its larger setup code
is another reason not to assume a win for tiny collections.

Nineteen focused cases cover scalar widths and floating-point bits, generic
schemas, module privacy, owner loans, nested enum storage, source evaluation
order, overflow/allocation failure recovery, and allocation-free reserved work.
The full workspace suite with all features, Clippy with all targets/features,
tutorial examples, Markdown links and the book build pass. The whole-row
performance limitation remains a follow-up concern.
