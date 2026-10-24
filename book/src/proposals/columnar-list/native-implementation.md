# Initial native column implementation

The [current interface](../../design/facilities/01-builtins/clists.md) implements
the bounded primitive-field version of `clist[T]`. The earlier
[generated-container experiment](implementation-spike.md) supplied the ownership
and performance baseline; its eight ordinary lists are not the native layout.

This page preserves the initial implementation's measurements. The subsequent
[indexed-access work](indexed-access.md) removes generic dispatch from field
access and row reconstruction; consult that report for the current comparison.

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
column plan. The initial indexed field access was checked through runtime helpers.
Strings, nested classes, enums, custom destructors and fallible initializers
are rejected. These restrictions make ownership and failure cleanup explicit
without committing to a future reference representation.

## Reproducing the comparison

Build without allocation instrumentation, then run the extended original probe:

```sh
cargo build --locked --release --bin plenty
python3 -B experiments/columnar-list/plenty/bench.py target/release/plenty \
  --native-clist --skip-generated --out /tmp/plenty-native-columns \
  --rows 8192 262144 --repeats 20 --samples 7 --cpu 8
```

The runner checks every checksum, discards the first sample, alternates layout
order, and retains both owners during measurement. This final comparison uses
native `clist` and ordinary `list[Particle]`; it omits the earlier generated
eight-list spike. The earlier three-owner run is superseded, so its absolute
timings are not mixed into this two-owner comparison.
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
compiler hash, frequency samples and host information. These measurements use
reviewed commit `d6e9a2cf` on an AMD Ryzen 7 7840HS, pinned to CPU 8 with the
performance governor, after competing builds and timings finished. Frequency
samples taken every 250 ms during execution ranged from 4.089 to 4.093 GHz;
before-run samples ranged from 3.271 to 3.511 GHz. This replaces the initial
throttled final numbers. It remains a workload-specific measurement on one host.

Median nanoseconds per row, over seven measured samples:

| Rows | Operation | Ordinary row list | Native clist |
| ---: | --- | ---: | ---: |
| 8,192 | One-field fold | 1.463 | 0.560 |
| 8,192 | Two-field filtered fold | 2.355 | 2.753 |
| 8,192 | All fields by index | 3.021 | 116.088 |
| 8,192 | Sequential whole rows | 3.047 | 96.809 |
| 8,192 | Permuted whole rows | 7.520 | 96.842 |
| 262,144 | One-field fold | 4.470 | 0.495 |
| 262,144 | Two-field filtered fold | 4.061 | 3.787 |
| 262,144 | All fields by index | 3.694 | 117.517 |
| 262,144 | Sequential whole rows | 3.971 | 96.098 |
| 262,144 | Permuted whole rows | 49.437 | 132.525 |

The ordinary all-fields and sequential whole-row controls borrow rows directly;
the column kernels respectively index fields and materialize rows. They perform
the same calculation using the available interfaces, so this compares practical
access paths rather than isolating memory layout alone.

The large one-field scan was about 9.0× faster than the row list. The two-field
median was only about 7% faster, with overlapping samples: native 3.660–3.845
ns/row and ordinary 3.648–5.448. This is not convincing evidence of a two-field
speedup; the small case was about 17% slower. The earlier 4.3× two-field claim
is withdrawn. Native columns use 64 bytes of payload per eight-i64 row, compared
with 128 tagged-slot bytes in the row list; one large native column occupies
2 MiB. These layout and code-generation benefits depend on the calculation.

Whole-row work is a substantial weakness of this first implementation:
sequential row copying was about 24× slower, and indexed eight-field work about
32× slower than the row-list controls. The native indexed kernel makes eight
generic dispatcher calls per row. Reconstruction walks the schema, recomputes
column prefixes and snapshots tagged row slots. These are current lowering
costs, not inherent costs of columnar storage. **Issue #432** records this
performance concern; the [follow-up](indexed-access.md) addresses that dispatch overhead.

Reserved construction took about 174 / 145 ns per row for the large
ordinary / native collections. These are single construction
samples, not a stable construction benchmark. They do not justify a general
append-speed claim.

## Generated code and correctness

`nm -S` on the large-case executable gives these kernel sizes in bytes, excluding
shared runtime helpers and measurement wrappers:

| Kernel | Ordinary row list | Native clist |
| --- | ---: | ---: |
| One-field fold | 161 | 294 |
| Two-field filtered fold | 235 | 620 |
| All fields by index | 362 | 1,269 |
| Sequential whole rows | 362 | 831 |
| Permuted whole rows | 560 | 936 |

The native one-field assembly has two setup helper calls, followed by a direct
eight-byte indexed load/add and the existing overflow check. There is no helper
call per element and no SIMD instruction in that loop. Its larger setup code
is another reason not to assume a win for tiny collections.

Twenty-one focused cases cover scalar widths and floating-point bits, generic
schemas, module privacy, owner loans, nested enum storage, source evaluation
order, overflow/allocation failure recovery, and allocation-free reserved work.
The full workspace suite with all features, Clippy with all targets/features,
tutorial examples, Markdown links and the book build pass. The whole-row
performance limitation motivated the subsequent indexed-access work.
