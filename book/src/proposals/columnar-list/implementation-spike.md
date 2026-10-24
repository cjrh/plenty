# Generated Plenty column-container spike

The primitive-only restriction makes a useful column container feasible with
Plenty's existing ownership rules. A generated facade passes actual allocation
failure and borrow checks, and a one-field scan is substantially faster than
ordinary row storage. It also exposes expensive indexed access and whole-row
reconstruction. These are reasons to implement direct column projection carefully,
not to ship the generated wrapper unchanged.

This 2026-10-24 follow-up to the [proposal](../columnar-list.md) uses Plenty
`00d32a13`. Sources, commands and preserved timing samples are in
`experiments/columnar-list/plenty/` in the repository. It changes no compiler,
runtime, standard-library API or tutorial behavior. `clist[T]` remains proposed.

## What was implemented

An experiment-only generator reads one public, nongeneric, field-only class.
It accepts `bool`, signed/unsigned 8/16/32/64-bit integers, `f32` and `f64`;
it rejects methods, destructors, private fields, nested/owned fields and empty
schemas. For `Particle` it emits a checked Plenty `ClistParticle` class with
one private list per field, length, reservation, append, scalar getters/setters,
whole-row copying, and borrowed column methods.

```text
mut items = ClistParticle()?
items.append(Particle(1, 2, 3, 4, 5, 6, 7, 8))?
xs = items.column_x()
total = iter(xs).fold(0, add)
```

This is specialization into existing language constructs, not a new parser
feature or an ordinary generic library implementation. Each column uses current
16-byte scalar list slots; the eight-field row list uses 128-byte row payloads.
Neither is packed native-word storage. Empty construction allocates eight list
handles; this differs from the proposed allocation-free empty owner.

Append reserves every column before publishing any value. A failure returns an
error containing the intact submitted row. Earlier capacity growth may remain,
but lengths and existing values are unchanged. Subsequent scalar appends cannot
allocate after reservation; the generator makes that invariant explicit with
`unwrap()`. Row reconstruction creates an independent value without allocation.
It never manufactures `&Particle` pointing across unrelated allocations.

## Ownership and failure evidence

The freshly built compiler with `runtime-checks` passes 35 generated fixtures
(31 executing programs and four compile-fail cases):

- Every empty-constructor allocation and every append reserve can fail, both
  before the first row and while growing eight existing rows. Failed append
  returns all eight input fields, and retry succeeds. Runtime allocation
  accounting finishes balanced on every exit.
- Reserved append, row reconstruction and a borrowed two-column `zip`/`fold`
  perform no successful allocation within checked regions.
- Differential tests against `list[Particle]` cover lengths 0, 1, 2, 7, 8, 9,
  33 and 1,025, field mutation, every field's value and negative indexing.
  Invalid row copies return `Nothing`, including extreme signed indices.
- All eleven accepted primitive types round-trip, including signed extrema,
  unsigned maxima and floating negative zero.
- Live column/pipeline loans reject owner mutation; moved owners cannot be
  reused; callers cannot mutate private backing lists independently.

Two generator unit tests reject unsupported schemas. The three original Plenty
feasibility probes still pass. No custom destructor, removal, stable row handle,
stored view, cross-thread operation or affine-field semantics are implemented.
Column loans conservatively borrow the entire owner.

Issue #427 records a frontend limitation found here: direct
`iter(items.column_x())` fails with `for requires an iterable`; binding the
returned reference first works. This restriction is not part of the desired API.

## End-to-end scalar measurements

AMD Ryzen 7 7840HS, x86-64 Linux, CPU 4, Plenty `--release`, current Cranelift
0.131.1. Eight checked `i64` fields per row; values repeat every 1,024 rows.
Both layouts remain allocated. Seven samples follow one warmup, each sample
repeats a scan 20 times, alternating layout order. Every checksum is checked
independently in Python. No cache flush, frequency lock, counters or non-x86
execution was used. These are warm scans on a shared machine.

Median nanoseconds per row; lower is better:

| Kernel | 8,192 rows: list | 8,192: columns | 262,144 rows: list | 262,144: columns |
| --- | ---: | ---: | ---: | ---: |
| One-field sum | 1.155 | 0.661 | 4.075 | 0.711 |
| Two-field filter/fold | 2.030 | 2.605 | 2.884 | 2.630 |
| All fields, direct indexed columns | 2.483 | 34.096 | 3.210 | 36.340 |
| All fields, `copy_row` sequential | 2.537 | 66.190 | 3.535 | 66.882 |
| All fields, `copy_row` permuted | 6.341 | 67.314 | 49.377 | 169.975 |

The clear result is the one-field sum: about 1.7× faster at the small size and
5.7× at the large size. Large rows occupy 32 MiB of payload, while one column
occupies 4 MiB; shared L3 is 16 MiB. Cache residency is part of the benefit.
The large one-column samples ranged 0.687–1.389 ns/row versus 3.556–4.651 for
the row list. Two-column results are mixed and close enough at the large size
that they do not establish a robust improvement.

The all-field comparison includes real facade costs, not just memory layout.
Assembly inspection shows:

- The one-column fold hoists length, base and stride, then directly loads and
  adds one scalar per iteration. There is no callback call in its hot loop.
- Two-column `zip` hoists both sources, but calls the scalar fold callback for
  each pair. Checked arithmetic and original evaluation order remain intact.
- Direct indexing of eight borrowed columns still calls
  `plenty_list_scalar_get` eight times per row. Issue #429 records this cost.
- `copy_row` adds a method call, reconstruction/Option handling and cleanup
  machinery. Avoiding reconstruction roughly halves the sequential cost here,
  but indexed access still dominates the direct-column version.

The reserved construction pass took 145 ns/row for the row list and 454 ns/row
for columns at the large size. This is one observation per layout, including
allocation and append, not a repeated growth benchmark. Eight reserve/check/
append paths per row are a visible cost of this generated implementation.

## Consequence for the design

Proceed with the primitive-only restriction and explicit borrowed column
operations. The spike validates their ownership model and demonstrates a useful
scalar scan benefit without SIMD or a backend switch. Ordinary list storage
remains preferable for the measured whole-row work.

A production implementation needs a deliberate representation and projection
path: packed primitive buffers if compact storage is promised, an empty owner
without eight heap handles, and direct checked loads instead of repeated helper
calls for recognized field scans. These findings concern frontend/runtime
representation as well as backend optimization; they do not establish a need
to replace Cranelift with LLVM. The remaining adoption scope belongs in
[B76](../../backlog.md), including removal, richer classes and portability checks.
