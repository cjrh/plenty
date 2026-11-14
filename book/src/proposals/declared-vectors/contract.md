# Declared-vector numeric and layout contract

**Unimplemented candidate contract for [declared vectors](../declared-vectors.md).**
The decisions below constrain a prototype; its public layout and API still
require evidence and approval. Current [`std.simd`](../../design/stdlib/simd.md)
continues to define its own implemented values.

## Scalar results in logical component order

For `Vec3`, logical order is x, y, z; `Vec4` adds w. Construction evaluates
component expressions once in written order. Each arithmetic operation uses
the scalar IEEE operation on its logical components. Lowering may combine
independent operations but may not reassociate them, contract multiplication
and addition to FMA, change rounding, or enable flush-to-zero/denormals-are-zero.
Optimized and fast-compile profiles have the same results at every CPU level.
NaNs, signed zeros, infinities and subnormals follow Plenty's scalar rules;
NaN payload identity is not newly promised.

Define ordered dot without an implicit initial `0.0`:

```text
Vec3: ((x * other.x) + (y * other.y)) + (z * other.z)
Vec4: (((x * other.x) + (y * other.y)) + (z * other.z)) + (w * other.w)
length(v): sqrt(v.dot(v))
```

This association matches the ordinary written three/four-term scalar
expression, including signed-zero behavior. Each product is rounded before
its addition; an FMA is a distinct operation and is not implied. Components
are processed in increasing order when a scalar fallback is used. A native
horizontal reduction must reproduce this association rather than use a tree
or backend-dependent order. No approximate reciprocal square root is included.

## Padding must do no extra floating work

A three-component value may use a four-lane native register. Its spare lane
is not a fourth scalar component, cannot be indexed or observed, and must not
introduce invalid operations, NaNs, subnormals or extra scalar side effects.
Canonical storage padding is positive zero. All constructors, loads,
conversions, copies, returns and component writes preserve that invariant.
Equality, formatting and any future hashing compare only logical components.

Merely zeroing the result is insufficient. For example, multiplying a zero
spare lane by a splatted infinity computes `0 * infinity` before it is masked.
Prepare the inactive operands with operation-specific neutral values before
the vector operation: a scalar multiplication uses `1.0` in the inactive
multiplier lane, not the scalar. Unary operations that would produce negative
zero in padding restore canonical positive zero. Reductions never include
padding. If a lowering cannot establish safe inactive operands, execute only
the logical components. Integer bit masking cannot retroactively undo a
floating exception or a slow subnormal operand.

This contract adds no public floating-environment control. The implementation
must nevertheless avoid extra exceptional operations from its private lane;
finite/NaN/subnormal/infinite scalar parameters all need disassembly and
behavior checks. `Vec4` has four real lanes and must retain the fourth lane's
normal behavior. Existing `f64x4` cannot silently acquire hidden padding.

## Candidate memory layout

Distinguish semantic payload, generic value slots and native registers. The
prototype must expose these costs separately rather than reporting a register
width as a complete class/list stride.

The candidate native payloads are 32 bytes with 16-byte alignment: x/y/z and
one zero padding word for `Vec3`, or four logical words for `Vec4`, each word
an IEEE `f64` in increasing component order. A 256-bit load may be unaligned
relative to a 32-byte register width; it must not assume 32-byte alignment.
Both payloads have the same layout at all CPU levels. A baseline target may
load the payload as two halves or three scalars. Declared fields retain their
names; public component writes maintain padding. Borrowing a component
materializes storage when necessary and obeys ordinary exclusivity/last-use
rules; it is not a reference into a fleeting vector register.

These payload widths require an explicit value-layout path. Current classes
and enums use [inline tagged slots](../../design/toolchain/02-architecture/values.md),
and existing SIMD values keep that ordinary public ABI with private vector
SSA. The experiment must report whether a local, enclosing class, tuple or
sum adds a 16-byte slot and how alignment/relocation work. Do not promise a
32-byte complete row merely because its payload is 32 bytes. The compact
candidate is to embed the payload directly in classes/arrays and use dense
32-byte list elements, with any enclosing owner bookkeeping outside it.
If achieving that requires a broader packed-value redesign than B83 supplies,
report it as a design cost and compare the ordinary-slot alternative before
acceptance. No proposal silently alters existing scalar class fields.

`clist` currently accepts only primitive fields. A vector field would need a
specific extension: x/y/z(/w) become separate primitive columns, with no
physical padding column, and copying a row reconstructs canonical vector
padding. `column` selection must distinguish a vector value from a scalar
component and state its yielded type. Whole-row and packed-field references
remain unsupported. Direct `clist[Vec3]` is the same three logical columns;
a containing `Body` with position/velocity fields needs deliberate flattening,
not blanket acceptance of arbitrary nested classes. Allocation failure,
capacity calculations and row-copy operations cover all columns atomically
under the current container contract.

A declared vector is copyable, owns no resource and needs no destructor.
Passing/returning it uses Plenty's private ABI; no C ABI, raw layout export,
serialization schema or native pointer reinterpretation is introduced.
Infallible conversions to/from owned inline arrays or tuples copy exactly the
logical components. Converting to `f64x4` explicitly supplies w (defaulting to
canonical `0.0` only if the public signature says so); converting back explicitly
discards w. It cannot leak a private padding word.

## Safety and plain-code behavior

Declared representation removes no bounds, overflow, index-normalization or
borrow check. Named component access is a fixed valid place; computed lane
access keeps the selected checked API. Array/list loads cannot read beyond
logical storage merely to fill a four-lane register. A compact external
three-double source must load three doubles and construct padding explicitly.

The pairing heuristic remains available for ordinary scalar code. A declared
vector selects its own lowering, but must not disable pairing in another
record, change unannotated tuple semantics or force every float spill in a
caller to a wider representation as a source contract. Register allocation
may still spill; measure its function-wide effects. The public guarantee is
semantic identity with an explicit vector abstraction, not a fixed instruction
count on every CPU.
