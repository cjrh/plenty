# User-defined dictionary keys and set members

**Status: Not implemented.** Nothing shipped. Dictionary keys and set members
remain integers, `bool`, `str`, and tuples of these, as the
[collections](../design/facilities/01-builtins/collections.md) and
[enums](../design/language/03-types/enums.md) references state. Equality stays
structural and non-overridable ([classes](../design/language/03-types/classes.md)).
No backlog entry exists yet.

## Problem

A program that wants `dict[Point, V]`, `set[Color]` or `dict[UserId, V]` must
flatten the key into a tuple by hand, losing the nominal type. Generic code is
worse off: an unconstrained `K` cannot appear in `dict[K, V]` at all.

```text
def count[K](xs: &list[K]) -> Result[dict[K, i64], AllocError]:
    ...
```

is rejected at the signature (`dictionary keys must be integers, bool, str, or
tuples of these`), before any instance is checked, so only `K: IntType` works.

The suggested direction is a `__hash__` dunder and a protocol requiring it as
the key bound. This page assesses that idea against the current implementation,
then recommends a narrower design.

## How dictionaries and sets work today

All dictionary and set operations enter one runtime function,
`plenty_collection(op, args, descriptor, site)`
(`plenty-runtime/src/aggregates.rs:1423`), with an opcode and the collection's
static type descriptor. Generated code never hashes or probes.

| Aspect | Current behavior |
| --- | --- |
| Descriptor | `Type` (`aggregates.rs:36`), 64 bytes, asserted layout: kind byte, `affine`, `reflexive`, inline payload size, key/value descriptors, name, variants (fields), and a `drop` function pointer at byte 56. Emitted by `src/codegen/metadata.rs`. |
| Hash | `hash(value, ty)` (`aggregates.rs:485`) interprets the descriptor recursively. Integers and `bool`: splitmix64 finalizer. `str`/byte strings: FNV-1a over the bytes. Kind `B` (tuples and enums): FNV-style xor-multiply over field hashes, seeded with the variant tag (issue #228). Any other kind falls through to the integer branch and would hash the value word, which for a class is its storage address. |
| Seeding | None. Hashes are deterministic across runs and processes. |
| Table | `Vec<usize>` of entry indices plus one, linear probing, power-of-two size, load at most one half (`try_reserve`, `aggregates.rs:546`). No hash is cached; rehashing and removal (`aggregates.rs:699`, backward-shift repair) recompute the home bucket of every key they visit. |
| Entries | Dense, type-sized rows (`plenty-runtime/src/entries.rs`). Inline payloads such as multi-field tuples live in the row. Iteration walks rows, not the bucket table, so dictionary order is insertion order regardless of hash values. |
| Equality | `equal(a, b, ty)` (`aggregates.rs:742`), structural, also used by `==` on classes, enums and collections (`src/codegen.rs:2481` lowers it to collection op 8). Floats compare by IEEE rules. |
| Ownership | `try_insert` retains copyable operands and adopts affine ones. Lookups take a value or reference. Borrowed iteration yields shared `&K`; mutable `items()` lends `&mut V` with shared `&K` ([iterables](../design/language/09-iteration/01-iterables.md)). No operation lends `&mut K`. |
| Admission | `Ty::hashable` (`src/collection.rs:189`): integers, `bool`, `str`, byte strings, and tuples of these. Checked where collection types resolve (`src/frontend.rs:885`) and where literals infer (`src/frontend/collections.rs:891`, `:3138`). |
| Callbacks | The runtime already calls generated code: a class's `__del__` adapter through the descriptor's `drop` slot, and executor/thread adapters with a three-pointer signature (`src/codegen/executors.rs`). Sorting has no user comparator; `compare` (`aggregates.rs:808`) is descriptor-interpreted like `hash`. |

The runtime relies on two properties of every legal key, recorded in
`try_reserve`: hashing never allocates and never calls user code. Table
rebuilding computes hashes alone, in the middle of a reservation, with no error
channel.

Two hash-quality defects exist independently of this proposal:

- FNV-1a has no finalizer. Multiplication carries bits only upward, so the low
  *k* bits of a string's hash depend only on the low *k* bits of each byte. A
  16-bucket table (the minimum) puts `"a"`, `"q"`, `"A"`, `"Q"`, `"1"` and
  `"!"` in one bucket. Tables below 256 buckets are affected.
- No seed: inputs that collide can be precomputed. `std.toml` indexes parsed
  keys in `dict[(i64, str), i64]` (`stdlib/std/toml.plenty:245`), so untrusted
  TOML can force quadratic parsing. The
  [dictionary reference](../design/facilities/01-builtins/dictionaries.md)
  already says adversarial collisions can make lookup linear.

### Measured cost of a composite key today

A probe on 2026-10-19 (release build of this branch, Linux x86-64, one pinned
core; the machine had load average about 18, so treat these as indicative) ran
20 million membership tests, half hits, minimum of seven runs:

| Entries | Empty loop | `dict[i64, i64]` | `dict[(i64, i64), i64]` |
| ---: | ---: | ---: | ---: |
| 1,024 | 0.03 s | 0.39 s (about 18 ns/test) | 0.92 s (about 45 ns/test) |
| 65,536 | 0.04 s | 0.70 s (about 33 ns/test) | 1.53 s (about 75 ns/test) |

A two-field tuple key costs about 2.4 times an integer key, including building
the probe tuple. Any user type admitted through the same descriptor path should
cost about the same as the tuple with the same fields. This must be confirmed by
the benchmark in [Measurements](#measurements).

## Precedents

**Python.** `__hash__` returns an integer and must agree with `__eq__`:
`a == b` implies `hash(a) == hash(b)`. Nothing enforces it. Defining `__eq__`
without `__hash__` makes a class unhashable; mutable built-ins are unhashable by
convention. A key mutated after insertion silently becomes unreachable.
Dataclasses derive both from fields (`frozen=True, eq=True`).

**Rust.** `Hash` and `Eq` are separate traits, usually derived together. A type
does not return a hash; it feeds its fields to a `Hasher`
(`fn hash<H: Hasher>(&self, state: &mut H)`). The collection chooses the hasher,
so seeding (SipHash with a random key in `std`), algorithm changes and mixing
quality stay out of user code. Consistency with `Eq` is a documented logic
requirement, not checked; violations cause wrong results but no memory
unsafety. Interior mutability makes key mutation possible (`Cell` in a key),
and is documented as a logic error.

| Shape | Distribution and seeding | User can break `a == b ⇒ hash(a) == hash(b)` |
| --- | --- | --- |
| `__hash__(self) -> u64` | User mixes bits; no seed; poor combinations (`x ^ y`) are easy | Yes, by hashing anything not determined by equality |
| `__hash__(self, h: &mut Hasher)` | Collection owns mixing and seed | Only by feeding non-structural data or by custom equality |
| Derived from fields | Runtime owns everything | No |

## Assessing the `__hash__` protocol idea

The idea needs several things Plenty lacks, and most of them work against the
project's goals.

1. **A protocol cannot express the bound.** Protocols match ordinary methods
   ([protocols](../design/language/07-generics/protocols.md)). Integers, `bool`,
   and `str` have no methods, so `dict[i64, V]` would not satisfy a method
   protocol unless the compiler special-cased it. Every user key type would also
   need a hand-written method even when the derived answer is the only correct
   one. A bound that admits built-ins and derived types is a built-in constraint
   like `IntType`, not a protocol.
2. **Custom hash without custom equality is nearly useless.** With structural,
   non-overridable equality, any pure function of the fields is a consistent
   hash. The only gain from a custom one is speed (hash an `id` field and skip
   the rest), at the price of a callback per hash. The cases people actually
   write `__hash__` for (case-insensitive names, ignoring a cache field) change
   *equality*.
3. **Custom equality is a larger change than hashing.** `==` would stop being
   structural for that type, `equal` in the runtime would need a callback, and
   collection equality, `in`, set algebra and dictionary comparison would all
   run user code. The [classes](../design/language/03-types/classes.md)
   reference promises equality "neither invokes user-defined magic methods".
   Allowing `__hash__` and `__eq__` separately also imports Python's unenforced
   consistency rule, which is exactly the kind of soft rule this language avoids.
4. **Callbacks break runtime invariants.** A user hash would run inside
   `try_reserve`'s rebuild, inside removal's cluster repair (once per displaced
   entry), and during set algebra. It would need a no-allocation, no-trap-on-error
   contract, an effect restriction, and a new descriptor slot. See
   [Where hashing runs](#where-hashing-runs).

The idea is right that keys need a named, generic bound. It does not need a
dunder.

## Where hashing runs

| | A. Descriptor-interpreted (today) | B. Runtime calls user hook | C. Generated code passes the hash in | D. Monomorphized table |
| --- | --- | --- | --- | --- |
| Mechanism | Extend `hash`/`equal` to classes | Descriptor gains hash (and equality) function pointers, called like `drop` | Each call site computes the probe hash and passes it to `plenty_collection` | Dict/set code generated per key type, in Plenty or codegen |
| Per lookup | Same path as tuples | Plus one indirect call and an adapter per hash and per probe equality | Saves descriptor walk for the probe key only | Inlined hash and equality, no dispatch |
| Rehash, removal | Recompute by interpretation | One callback per visited entry | Needs cached per-entry hashes, since the runtime cannot hash stored keys | Inlined |
| Memory per entry | Unchanged | Unchanged | Plus 8 bytes per row or per bucket | Unchanged or less |
| Code size, compile time | One descriptor per key type, already emitted | One adapter function per key type | Hash code at every call site or one helper per type | Whole table per instantiation; large |
| Runtime invariants | Kept: no allocation, no user code | Broken; needs effect and allocation rules | Kept for hashing; equality still interpreted | Must re-implement reservation, failure, destruction-resume and removal semantics in generated code |
| Consistency with `==` | By construction: one `equal` | Only if equality is also the hook's | By construction if derived | By construction if derived |

Measured context: in-cache integer lookups cost about 18 ns, almost all of it
call, dispatch, and probing rather than hashing an integer. A callback adds an
indirect call, an adapter that unpacks the slot, and possibly a `Hasher` object;
expect a few nanoseconds per hash and per equality probe, on top of the
tuple-equivalent cost. That estimate needs the measurement below before B is
considered.

D gives the best per-operation speed and the worst compile time, code size, and
correctness surface. It would duplicate `aggregates.rs`'s allocation-failure
and destruction-order guarantees per type. It is the right tool only if the
dispatcher itself, not hashing, dominates. Issue #18 gave list reads narrow
runtime helpers for that reason; the same approach could apply to hashed
lookups for all key types at once.

## Recommendation

**Derived structural keys, a built-in `KeyType` constraint, and no user hash
or equality hooks.** The rule has no exceptions and cannot be violated:

> A key type is an integer, `bool`, `str`, byte string, or a tuple, class, or
> enum whose every field or payload is a key type. Recursive types are not key
> types. Dictionary keys, set members, and `KeyType` arguments must be key
> types. Hashing and equality are structural, so `a == b` always implies equal
> hashes.

```text
class Point:
    x: i64
    y: i64

enum Shape:
    Dot(Point)
    Segment(Point, Point)

def count[K: KeyType](xs: &list[K]) -> Result[dict[K, i64], AllocError]:
    ...

seen: set[Shape] = set[Shape]()?          # accepted
weights: dict[Point, f64] = {}?           # accepted: values may be floats
bad: set[Sample] = set[Sample]()?         # rejected if Sample has an f64 field
```

### Rationale

- **Consistency is impossible to break.** One structural `equal` serves `==`
  and lookup; one structural `hash` reads the same fields. There is no
  `__hash__` to forget, no `__eq__` to disagree with it.
- **No new runtime invariant.** Hashing still never allocates or calls user
  code, so `try_reserve`, removal repair, and worker-thread eligibility need no
  new rules. Effects are `Compute` only by construction
  (`src/effects.rs`).
- **Small change.** The runtime already hashes kind `B` structurally; it needs a
  class branch. The frontend needs one predicate, one constraint, and better
  diagnostics. Existing keys keep identical code paths.
- **Custom identity stays explicit.** A case-insensitive key is a key built
  from the normalized text (`Name(text.lower()?)`), not a hidden equality. The
  allocation is visible at construction, not repeated per probe.

### Key immutability

Keys cannot change while stored, by existing rules plus the key-type rule:

- Insertion moves an affine key in or copies a copyable one.
- Every read of a stored key is a shared `&K`; no API lends `&mut K`, including
  mutable `items()` views and `replace(d[k], v)`, which targets values.
- Key types contain no interior mutability. Excluded by the rule, without a
  separate list: floats (`NaN != NaN`, and `0.0 == -0.0` with different bits),
  references, `Box` and recursive data, collections, closures and callables,
  generators, atomics (`std.atomic`), `std.sync.Mutex`, channels,
  `CancellationToken`, tasks, files, sockets and other handles. A class holding
  any of these is not a key type.
- Strings share immutable storage, so retaining a `str` key is safe.

### Affine keys

Every class is affine
([moves and copies](../design/language/05-ownership/01-bindings-moves-and-copies.md)),
so class keys are the first affine keys. Operations that today retain keys into
new storage need the same treatment affine *values* already have:

| Operation | Affine key rule |
| --- | --- |
| `insert`, `add`, `update` | Already adopt affine operands |
| `get`, `[]`, `in`, `pop`, `discard` | Accept `&K`; an owned temporary is consumed |
| Borrowed iteration, `items()` | Already yield `&K` |
| `keys()` | Mirror `values()`: owned temporary, or `copy(d)?.keys()` |
| `union`, `intersection`, `difference`, `symmetric_difference` | Need copied members (fallible, already `Result`) or a consuming form; decide per operation |
| Set and dict comprehensions | Move keys in as for `insert` |

Copyable enums (payloads of copyable key types) avoid this table entirely, which
motivates the staging below.

### Generic bound

`KeyType` is a built-in constraint beside `IntType` and `FloatType`
(`src/frontend/generics.rs:19`). In a generic signature, `dict[K, V]` and
`set[K]` require `K: KeyType`; today's eager rejection at resolution stays, with
a diagnostic that names the fix. At a concrete use, a rejected type reports the
first offending path:

```text
error: `Sample` is not a key type
  note: field `Sample.ratio` has type `f64`; floats are not key types
```

```text
error: `dict[K, i64]` requires `K: KeyType`
  help: declare `def count[K: KeyType](...)`
```

### DoS resistance and seeding

Separate from user keys, but the Hasher-style design above keeps it a runtime
decision. Recommended, in one change after key types land:

1. Add a finalizer (splitmix64) to string and composite hashes, fixing the
   low-bit defect for small tables. Cost: a few cycles per hash.
2. Seed the hash from OS entropy once per process (`plenty-runtime/src/entropy.rs`
   exists), or per table. Dictionary iteration order does not depend on hashes,
   so the only observable change is set iteration order where it already is
   unspecified. A deterministic mode for reproducible tests is an open question.
3. Consider a keyed hash (SipHash-1-3, or a faster keyed hash) for strings only
   if measurements show the seeded FNV/finalizer path is still attackable;
   seeding a non-cryptographic hash does not stop all collision attacks.

## Costs

| Cost | Recommended design |
| --- | --- |
| Lookup, user key | Same descriptor path as a tuple with the same fields; about 45 ns versus 18 ns for `i64` in the probe above |
| Existing keys | Unchanged until the finalizer/seed change, which adds a few cycles per hash |
| Per field | One recursive `hash`/`equal` call. `Record::slot` (`aggregates.rs:188`) recomputes each offset from the start, so hashing or comparing an *n*-field record is O(*n*²) descriptor reads; walking offsets incrementally fixes it for tuples and classes alike |
| Memory per entry | Unchanged: 8-byte bucket at load at most one half, plus a row sized from the key's inline payload, as for tuples |
| Code size, compile time | One static descriptor per key type, which collection types already emit; no per-type functions |
| Rehash | Every resize recomputes all hashes by interpretation; composite keys pay more than integers |

Caching hashes is not part of the recommendation. It would cut rehash and
removal cost and give a cheap pre-equality filter, for 8 bytes per bucket
(doubling the table) or per row. Measure before adding it.

### Measurements

Before implementation and before any follow-up optimization:

1. Extend `examples/collection_bench.py` (method in
   `examples/collection-bench.md`) with insert, hit, miss and
   remove workloads at 1,024 and 65,536 entries for `dict[i64, i64]`,
   `dict[(i64, i64), i64]`, `dict[Point, i64]` (two `i64` fields), a copyable
   enum key, and `dict[str, i64]` with short keys. Pin to physical cores as in
   [profiling](../development/profiling.md).
2. Rerun after the finalizer and seed change; report per-operation deltas for
   existing key types.
3. To evaluate design B at all: prototype a descriptor hook that returns the
   structural hash through an adapter and compare against A on the same
   workloads.
4. Compile time: `compile_bench -- 1000 10 --workload aot` before and after,
   per [profiling](../development/profiling.md); expected neutral.

## Rejected alternatives

- **`__hash__(self) -> u64`.** User mixing, no seeding, and consistency is
  unenforced. Adds a callback for no correctness gain under structural equality.
- **`__hash__(self, h: &mut Hasher)`.** Fixes distribution and seeding, but the
  only benefit without custom equality is skipping fields, paid for with a
  callback per hash and new runtime invariants.
- **`__eq__` plus `__hash__`.** Makes `==` non-structural, puts user code inside
  every collection algorithm, and reintroduces a consistency rule the compiler
  cannot check.
- **A method protocol as the bound.** Cannot admit built-ins without a special
  case and forces boilerplate on every key type.
- **Monomorphized tables (D).** Compile time, code size, and duplicated failure
  semantics, against fast compilation. Revisit only if dispatch, not hashing,
  dominates measured cost, and then prefer narrow runtime helpers.
- **Hashing floats by bits.** Breaks `a == b ⇒ hash(a) == hash(b)` for
  `0.0`/`-0.0` and makes `NaN` keys unreachable. A user who needs float keys
  can store a canonical integer encoding explicitly.

## Possible later extension: key projection

If custom identity proves necessary, the consistent form is one method that
defines both equality and hashing by returning a key-type value:

```text
class Name:
    text: str

    def __key__(self) -> str:
        ...  # normalized text
```

`==`, `hash`, `in` and lookup would all compare `__key__()` results, so they
cannot disagree. It still needs a runtime callback (design B), a
no-allocation rule on the projection (normalizing text usually allocates, which
defeats the motivating example), and a change to the structural-equality
promise. Explicit key construction covers the same cases today; this is not
recommended now.

## Ordering and sorted maps

Out of scope. [User-defined ordering](user-type-ordering.md) faces the same
choice for `<` and `sort`: `compare` (`aggregates.rs:808`) is descriptor-
interpreted like `hash`, and the same derived, structural rule keeps ordering
consistent with equality. A sorted map would need both bounds; if ordering is
derived structurally, every key type with no float fields is also totally
ordered, and the two constraints could share one admission walk.

That proposal recommends a `__cmp__` hook that also defines `==`. A type that
defines `__cmp__` is therefore not a key type: its equality is user code, so a
structural hash would not agree with it. Such values are keyed by a projection
of the fields their `__cmp__` compares.

## Open questions

1. Name: `KeyType`, `Hashable`, or `Key`. `KeyType` matches `IntType`.
2. Should classes with `__del__` be key types? Equality is structural on them,
   but keys are dropped during table destruction and removal; copying is
   impossible, so `keys()` and set algebra are unavailable.
3. Private fields: structural hashing reads them, as equality already does.
   Confirm that is acceptable for public key types across modules.
4. Seed scope: per process or per table, and whether a test-mode environment
   variable fixes the seed.
5. Whether `std.report`-style anonymous sums should be key types when every
   member is.
6. Whether to cache hashes, after measurement.

## Implementation sketch

1. **Copyable enums.** Extend `Ty::hashable` (`src/collection.rs:189`) to user
   and standard enums whose payloads are key types, rejecting recursion through
   `type_facts` (`src/type_facts.rs`). The runtime path already works for kind
   `B`. Update the three diagnostics (`src/frontend.rs:885`,
   `src/frontend/collections.rs:891`, `:3138`) to name the offending field.
   Tests in `tests/test_collections.rs`, `test_dictionary_lookup.rs` and
   `test_set_operations.rs`; reference updates to `collections.md`, `enums.md`,
   `sets.md` (which still lists only integers, booleans, and strings) and
   `tutorial/04-collections/dictionaries.md`.
2. **`KeyType` constraint.** Add it to the built-in constraint table
   (`src/frontend/generics.rs:19`, diagnostic at `:1001`); let `dict[K, V]`
   resolve in generic signatures when `K: KeyType`. Update
   [generics](../design/language/07-generics/index.md).
3. **Classes and affine enums.** Add a class branch to `hash`
   (`plenty-runtime/src/aggregates.rs:485`) over `Record` fields, plus a unit
   test beside `aggregates/hash_tests.rs` that equal classes in different
   storage hash equally. Audit each operation in [Affine keys](#affine-keys) in
   `src/frontend/collections.rs` and the runtime's retain/release paths
   (`try_insert`, `Collection::at`, set algebra). Fix `Record::slot`'s quadratic
   offset walk.
4. **Hash quality and seeding.** Finalizer, then seed; benchmark per
   [Measurements](#measurements). Update the dictionary reference's collision
   sentence.
