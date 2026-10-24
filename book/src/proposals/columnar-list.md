# Columnar lists with an object-shaped interface

> **Status: Not implemented as a language feature.** Layout/ownership experiments
> now include a [generated Plenty implementation spike](columnar-list/implementation-spike.md)
> based on `00d32a13` on 2026-10-24. `clist`, field selectors,
> and column views below are proposed syntax. No compiler or runtime feature
> ships here. [B76](../backlog.md) records the candidate work.

Recommend an explicit `clist[T]` storage choice for repeated scans of a few
fields of many records. Keep ordinary class declarations and ordinary
`list[T]` unchanged. Start with flat, fixed-width scalar fields and a small
indexed-field/column interface; earn a richer object facade through separate
ownership and code-generation evidence. Do not require an ECS, dataframe
query language, automatic parallelism, or a new backend to try this idea.

```text
class Particle:
    x: f64
    y: f64
    mass: f64
    kind: i64

mut particles = clist[Particle]()
particles.append(Particle(1.0, 2.0, 3.0, 0))?
particles[0].x = 4.0
```

This is a storage decision at the collection declaration, not a change to
`Particle` everywhere. The spelling `clist` is provisional. The empty owner
should require no allocation; the first append is fallible. Proposed append
errors retain the submitted row; see the [ownership and API contract](columnar-list/contract.md).

The [original layout evidence](columnar-list/evidence.md) and
[Plenty implementation spike](columnar-list/implementation-spike.md) support the
narrow-field use case and expose tradeoffs. The Plenty experiment establishes
bounded scalar scan benefits with ordinary list-backed columns, not packed
primitive storage, a final chunk size, or safe general row references.

## What changes in memory

For four particles, ordinary array-of-structures (AoS) storage groups each
record's fields. Structure-of-arrays (SoA) groups each field's values:

```text
AoS:  [x0 y0 m0 k0] [x1 y1 m1 k1] [x2 y2 m2 k2] [x3 y3 m3 k3]
SoA:  x: [x0 x1 x2 x3]     y: [y0 y1 y2 y3]
      m: [m0 m1 m2 m3]     k: [k0 k1 k2 k3]
```

Scanning `mass` then brings useful values into cache without the other fields.
This concerns cache lines, prefetching and memory bandwidth; it need not involve
OS paging. Contiguous columns also make ordinary scalar loops simpler and give
vectorization a better input representation. Neither SIMD nor multithreading
is implied by choosing the layout.

Whole-row access is not uniformly slower. Sequential full-row work can stream
several columns efficiently, and compiler vectorization can change the result.
Random whole-row access often needs several unrelated cache lines. Appending
touches more buffers but can remain amortized constant time; deletion costs
depend on ordering and identity promises. Measure the actual workload rather
than naming a universally superior layout.

Plenty's current runtime already keeps inline class payloads in type-sized
collection rows (`plenty-runtime/src/entries.rs`); it is not simply a list of
heap-object pointers. Its `u128` storage words and bookkeeping also differ
from tightly packed native structs. A clist implementation must decide how
packed scalar columns cross that representation boundary. Reusing eight
ordinary lists is a useful semantic prototype, not evidence of packed columns.

## Learn from adjacent systems

| Precedent | Useful part | Boundary for Plenty |
| --- | --- | --- |
| [Arrow columnar format](https://arrow.apache.org/docs/format/Columnar.html) | Struct arrays have equal-length child arrays; primitive columns support sequential scans | Arrow is an in-memory interchange layout, not an ownership checker. Adopting SoA alone does not give Arrow-compatible strings, validity bitmaps, alignment or zero-copy interchange. |
| [Parquet row groups](https://parquet.apache.org/docs/concepts/) | Divide rows into groups, each containing a chunk per column | Parquet describes a file format. Its disk-oriented group sizes and compression/page machinery are not defaults for an in-memory container. |
| [Flecs archetypes](https://www.flecs.dev/flecs/FAQ.html) | Group entities with the same components for efficient iteration | Component columns need not split each component's fields. A fixed-schema clist needs neither dynamic archetype migration nor an entity scheduler. |
| [soa-rs](https://docs.rs/soa-rs/1.0.0/soa_rs/trait.Soars.html) | Separate generated row-reference and column-slice types from owned rows | Its references contain references to individual fields. Plenty currently forbids stored aggregate references; this cannot be copied as an ordinary library class today. |
| [DuckDB zonemaps](https://www.duckdb.org/docs/current/guides/performance/indexing) | Conservative min/max bounds can skip groups, especially when values are clustered | Arbitrary Plenty callbacks can have effects and traps. Skipping their execution needs a recognized safe predicate or an explicit query operation. |

## Small initial scope

The initial candidate is a compiler-recognized collection for a fully concrete
class whose fields are integers, floating-point numbers or booleans, with no
custom destructor. Normal construction may happen before insertion; clist does
not rerun a constructor per field. Custom constructors do not automatically
justify exposing otherwise private fields. Exclude nested aggregates, text,
collections, owned closures and recursive fields until their movement and
destruction contracts are proved.

Provide length, fallible reserve/append, scalar field reads and writes,
explicit whole-row materialization/removal, and a typed immutable column
projection feeding existing fluent operations. Do not add a second family of
`map_field`, `filter_field`, and `reduce_field` methods. The detailed
[contract](columnar-list/contract.md) makes the incomplete facade visible.

Start with dense flat columns. Preserve insertion order; use explicit ordered
removal and defer cheap tombstones. Keep physical storage private so fixed-size
chunks can be evaluated later without changing the element type. The
[chunking and deletion assessment](columnar-list/chunks.md) records how to
extend it without turning `clist` into a database prematurely.

## Fit with pipelines and SIMD

Column projections should be borrowed sources for the existing
[fluent pipeline model](fluent-iterators.md), with `zip` for multi-column work.
The source carries type, length, stride and ownership information; callbacks
keep ordinary evaluation order, traps and allocation behavior. A projection
must not allocate a temporary list. The first implementation may use scalar
loads and still benefit from fewer fetched bytes.

The [SIMD proposal](std-simd.md) can later recognize safe kernels over these
sources. Integer overflow must retain Plenty's checked semantics unless the
program explicitly uses wrapping operations; floating reductions stay ordered.
Contiguous storage does not authorize reassociation, speculative callback
execution, or automatic threads. The original layout experiment uses
LLVM-generated native kernels; the implementation follow-up uses Plenty and
Cranelift. Neither is a controlled backend comparison. The existing SIMD
evidence remains the backend comparison. Avoid building a dataframe optimizer
inside Plenty simply to compensate for a backend limitation.

## Adoption gate

There is no demonstrated fundamental layout blocker. There **is** a substantial
semantic boundary between cheap field access and a transparent `list[T]`
replacement. Before accepting production implementation, require:

- A small generated layout path whose scans touch only selected columns,
  with bounded compile-time and code-size cost per class specialization.
- Scalar differential tests against ordinary lists, including empty/tail
  inputs, checked arithmetic, field mutation and ordered removal.
- Allocation failure at every required reserve, unchanged logical contents,
  exactly-once destruction, and useful unsupported-element diagnostics.
- Compile-fail borrowing tests for mutation/reallocation with live projections,
  private-field access, whole-row borrowing and callback escape.
- End-to-end Plenty benchmarks for narrow and full-row work, growth and
  deletion, including small inputs and at least one non-x86 execution target
  before making portable performance claims.

The proposal is a reason to try that bounded implementation, not a commitment
to the later row facade, stats, vacuum, or interchange APIs.
