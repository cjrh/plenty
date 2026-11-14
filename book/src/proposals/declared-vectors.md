# Declared vectors for related floating-point components

**Status: Approved investigation design; vectors are not implemented, 2026-11-14.**
Local issue #967 requests this comparison before adopting a vector type.
The B83 [inline-array prerequisite](../design/language/03-types/arrays.md) has
shipped in local PR #1262. The [bounded experiment](../completed-work/performance.md#declared-vector-prototype-issue-967),
local PR #1263, is delivered and has not passed its adoption speedup gate. Approval of this
proposal accepts the investigation contract, not a public API, physical layout
or production speedup. Current behavior remains the [scalar row heuristic](../design/toolchain/02-architecture/numerical-loops.md#rows)
and explicit [`std.simd`](../design/stdlib/simd.md). B23/B83 in the
[backlog](../backlog.md) retain the remaining scope.

## The problem and intended use

Positions, velocities and colors are often computed componentwise. A storage
record with unrelated floating fields does not express that relationship.
Today's optimizer guesses pairs from adjacent isomorphic statements; a source
edit can prevent pairing, and three components remain a pair plus a scalar.
A declared vector would express one logical numeric value independently of
that guess.

Use a declared vector when its small group of components is normally computed
together. Keep ordinary fields when they describe unrelated quantities, are
usually read one at a time, or need independently addressable storage. Packing
can add lane extraction, materialization, padding and dependencies. It does
not guarantee a speedup, especially for subnormal operands or register-heavy
functions.

A declaration would guarantee componentwise scalar floating semantics and
make a value eligible for one native vector where the target supports its
width. It cannot guarantee a physical register for an escaped address, a call
boundary, or a function under register pressure. At baseline x86-64, three
`f64` components require several native parts; at x86-64-v3 a four-lane
register is available. Target selection remains explicit and affects only
lowering, never results.

## Alternatives

| Candidate | Strengths | Costs and unresolved boundaries |
| --- | --- | --- |
| Dedicated `std.vector.Vec3`/`Vec4` domain values, initially `f64` | Named components, conventional operators and explicit ordered reductions; distinguishes geometry from raw storage; can document one stable semantic contract. | Compiler-known representation and collection layout are needed for an efficient implementation; creates a new module and requires explicit conversions. |
| An annotation on an ordinary class, such as `@vector` | User-chosen names; fits a `Position` record and can preserve nominal domain identity. | Must constrain field types/count/order, arbitrary methods, field references, generics and lifecycle; changing an annotation changes representation. Hidden layout restrictions make it a larger language feature. |
| Add `std.simd.f64x3` | Extends an existing closed family and scalar fallback; explicit native lowering already exists. | Lane-count interface does not name `x`, `y`, `z`; three logical lanes still need padding rules and collection support. It is a kernel primitive rather than a domain value. |
| Use only `array[f64, 3]` | One shared implemented layout/literal/indexing mechanism; useful for generic storage and fixed body counts. | Storage alone gives no componentwise operator or ordered-dot contract, no semantic distinction from unrelated entries, and no guarantee values remain native through arithmetic. |

**Recommended prototype candidate: dedicated `Vec3` and `Vec4` values backed
by the same inline-storage mechanisms as arrays.** Keep their scalar
implementation in Plenty; compiler lowering can preserve native vectors where
proved equivalent. Do not add an annotation or teach arrays to imply geometric
arithmetic. Public names and exact physical layout remain provisional until
the prototype demonstrates the end-to-end case. An initial candidate needs
only `f64`; generic lane counts and additional scalar types are separate API
decisions, not an automatic public extension.

The existing `std.simd` types retain their full logical lane semantics. A
`Vec3` cannot be a transparent alias for `f64x4`, because its fourth lane is
not user data. A tuple remains storage. Every conversion between these types
is explicit and states whether a fourth lane is supplied, checked or discarded.

## Candidate surface

The following is hypothetical API notation, not accepted Plenty declarations:

```text
from std.vector import Vec3, Vec4

position = Vec3(x, y, z)
velocity = Vec3(vx, vy, vz)
position = position + velocity * dt
squared_distance = (position - other).dot(&(position - other))
```

The initial surface is construction, named component reads/writes, checked
indexed access, `+`, `-`, unary `-`, scalar `*` in both orders, componentwise
`mul`, `dot` and `length`. Float operations allocate nothing and have their
ordinary IEEE outcomes, not arithmetic `Result` wrappers. `dot(other)` and
`mul(other)` borrow their read-only operands. `length()` is a geometric
magnitude; the value is not an iterable container exposing `len` as magnitude.
Normalization and division are unnecessary for the first measurement and are
deferred rather than given an implicit zero-vector policy.

Conform to the [standard-library conventions](../design/stdlib/conventions.md)
when signatures are finalized: borrowed read-only inputs, no allocation caps
for fixed-size allocation-free values, named explicit conversions, `i64`
indices, formatting through the normal formatter hooks, and all public
operations documented. The sibling SIMD `lane(index) -> Option[f64]` is the
candidate checked-index spelling; no raw public native hook or unchecked
component access is introduced. No external library or runtime wrapper is
needed for these small scalar operations.

The [numeric and layout contract](declared-vectors/contract.md) specifies what
the candidate must preserve. The [prototype and evidence gates](declared-vectors/evidence.md)
state how to decide whether it should ship.

## Sequence and approval boundary

B83 inline arrays and their construction/layout rules are delivered. This
approved proposal defines the bounded experiment: compare the candidate
against those arrays and current scalar/std.simd forms, then review the
results before committing a public design. The separate experiment PR #1263
records the bounded candidate and its negative adoption result. The pairing heuristic for unrelated
plain code remains unchanged.

No new prototype, compiler change, benchmark rewrite, public std declaration
or performance result is delivered here. Existing measurements inform the
experiment; they do not satisfy its gate. Reference/tutorial examples of the
new type belong in a feature PR only after that design is accepted and
implemented. Their required before/after example must include costs and
measured results, not merely attractive vector notation.
