# Value generic parameters

**Status: Not implemented.** Nothing shipped. Generic parameters are types only;
the [generics reference](../design/language/07-generics/index.md) is
authoritative. No backlog entry exists yet; the closest rows are B13 (bounded
and inline-capacity storage) and B23 (SIMD). Researched against local `main` at
`1b5b96b1` on 2026-10-27. Per-instance assertions depend on the module-scope
`assert` in local PR #628, which is not yet merged.

## Problem

Plenty cannot write a type whose size is a compile-time number. The language
has inline aggregates (classes, tuples and enums store their fields inline since
B35), but every inline aggregate has a fixed field list written out by hand.
Code that wants "N values of `T`, stored inline" must either allocate a `list`
or spell out N fields.

`std.simd` shows the cost. Its six types (`i32x4`, `u32x4`, `f32x4`, `f64x2`,
`mask32x4`, `mask64x2`) take 614 lines of `stdlib/std/simd.plenty`. Each stores
its lanes as enum payload fields, `Lanes(i32, i32, i32, i32)`, so every method
destructures both operands by name, and `lane(index)` is an `if` chain over the
fields because inline storage cannot be indexed:

```text
pub def lane(self, index: i64) -> Option[i32]:
    match self:
        case i32x4.Lanes(a0, a1, a2, a3):
            if index == 0:
                return Some(*a0)
            ...
```

The [memory proposal](memory-parallelism-and-simd.md#fixed-capacity-and-inline-storage)
names `Array[T, N]`, `BoundedList[T, N]` and `SmallList[T, N]` as later types and
defers them "until inline aggregates and basic constant parameters exist".
Inline aggregates now exist. This page designs the constant parameters and
the array, spelled `array[T, N]` to match `list` and `tuple`.

## What exists today

| Aspect | Current behavior | Source |
| --- | --- | --- |
| Parameters | `[T]`, `[T: IntType]`, `[T: FloatType]`, protocol and `Callable` bounds. Each is a type. | `type_parameters` in `src/frontend.rs` |
| Substitution | A name-to-`TypeRef` list; a body is substituted per instance | `Substitution`, `substitute` in `src/frontend/generics.rs` |
| Checking | Bodies are checked per used instance, not at declaration; signatures and bounds at declaration | [Generics](../design/language/07-generics/index.md#diagnostics-from-instances) |
| Inference | All arguments explicit, or all inferred structurally from inputs; output-only parameters need an explicit list; no number widening | [Inference](../design/language/07-generics/inference.md) |
| Identity | Different arguments are different nominal types, even for phantom parameters | [Generic data](../design/language/07-generics/data.md) |
| Limits | 16,384 instances per generic function or data type; 262,144 function and 65,536 data instances per compilation; dependency depth 256 plus the number of generic declarations | [Emission](../design/toolchain/02-architecture/emission.md), `src/frontend/generics.rs`, `src/frontend/data.rs` |
| Brackets in expressions | `base[...]` that parses both ways is resolved by name: a local is indexed, a declaration takes type arguments | `Expression::Ambiguous`, `src/frontend/modules.rs` |
| Module constants | `const name: T = value`, integer or float, literal or numeric type constant initializer; each read becomes an immediate | [Modules](../design/language/02-source/modules.md), [call-site substitution](../design/toolchain/02-architecture/emission.md#call-site-substitution) |
| Fixed small groups | Tuples are inline; `t[0]` needs a literal index | [Tuples](../design/language/03-types/tuples.md) |

Phantom type parameters already give nominal tags: `Quantity[Meters]` and
`Quantity[Seconds]` are distinct types. Value parameters add two things phantom
types cannot: storage sized by the value, and the value itself in the body
without a runtime field.

## Where value parameters would and would not help

| Code | Today | With value parameters |
| --- | --- | --- |
| `std.simd` lane types | Hand-written payload fields per shape; native lowering matches six names (`src/codegen/portable_simd.rs`) | An internal `Lanes[T, const N: i64]` over `array[T, N]` could share bodies, with the public names kept as aliases. Lowering would match instance identities. The [SIMD API](std-simd/api.md) keeps a closed public set; that decision stands. |
| Bounded containers (B13) | Not expressible without allocation | `BoundedList[T, const N: i64]` over inline storage reports capacity exhaustion without allocating |
| `std.archive` 512-byte tar blocks (`block_size`) | Heap `Bytes` | `array[u8, block_size]` once reads can target a borrowed byte view (B11/B39) |
| `std.random` `state() -> (u64, u64, u64, u64)` | Tuple | No real gain: tuples cover small fixed groups in non-generic code |
| `std.sketch` `HyperLogLog` precision | Runtime field, validated, checked on merge, read from serialized data | Worse: `from_bytes` reads the precision from data, so it must stay a runtime value |
| `std.crypto` digest size | Runtime `Algorithm`; `finish_into` writes into a caller buffer without allocating | No gain: the algorithm is a runtime choice and the allocation-free path exists |
| `clist` chunks | Physical storage kept private so chunk size can change ([chunks](columnar-list/chunks.md)) | Not wanted: a public chunk-size parameter would fix what the design keeps private |

Every case that benefits needs indexable inline storage. Value parameters
without an array type would only save a field. They should ship with
`array[T, N]`, not before it.

## Precedents

| Language | Parameter types | Argument expressions | Inference | When checked |
| --- | --- | --- | --- | --- |
| Rust (stable, `min_const_generics`, 1.51) | Integers, `bool`, `char` | Literal, bare const parameter, or a `{ }` block independent of generic parameters | From `[T; N]` in argument types | Generically, before monomorphization |
| Rust (`generic_const_exprs`, unstable) | Same | `N + 1` and other expressions over parameters | Not through expressions | Needs `where [(); N + 1]:` evaluatability bounds |
| C++ non-type template parameters | Integers, enums, pointers; floats and structural classes since C++20 | Any constant expression | Deduced only from bare `N`; `N + 1` is a non-deduced context | At instantiation |
| D template value parameters | Any type usable at compile time | Any CTFE-evaluable expression | From argument types | At instantiation; `if (n > 0)` constraints |
| Zig `comptime` parameters | Any type, including `type` | Arbitrary compile-time evaluation, bounded by an eval-branch quota | Explicit arguments; types are values | Lazily, per instantiation |
| Mojo parameters | Any compile-time type | Arbitrary parameter expressions | From arguments | Per instantiation; `SIMD[DType.float32, 4]` is the stdlib vector |

Lessons:

- Rust's stable subset has served for years; the hard part is expressions. With
  `generic_const_exprs`, `N + 1` and `1 + N` do not unify, well-formedness needs
  evaluatability bounds, and the feature has stayed incomplete. The Rust project
  has since pursued narrower argument forms. Plenty should not start there.
- Before const generics, Rust implemented array traits only for lengths 0 to 32.
  That is the duplication `std.simd` shows today.
- C++ and Zig show that arbitrary compile-time evaluation produces late, long
  diagnostics and needs evaluation budgets.
- Plenty differs from Rust in one useful way: bodies are already checked per
  instance. Inside a body `N` is a known number, so arithmetic on it is ordinary
  arithmetic on a constant. Only type positions need restriction.

## Options

| Option | Benefit | Cost |
| --- | --- | --- |
| A. Status quo: concrete types, hand-written or generated | No language change | Duplication (`std.simd`); no bounded containers without allocation |
| B. Runtime capacity fields | Works today (`BufReader`, `HyperLogLog`) | Heap storage; checks at run time; no inline arrays |
| C. Minimal value parameters: integer and `bool` values, bare arguments only | Covers arrays, bounded containers and fixed lanes; no expression unification; finite instance set | No `array[T, N + 1]` in signatures |
| D. C++-style: expressions in non-inferred type positions, evaluated per instance | `concat` and `push` can name result sizes | Errors appear at the use, inside the callee's signature; more checking machinery |
| E. Zig-style compile-time evaluation | Most expressive | A compile-time interpreter, evaluation budgets, and generic code that is hard to read; against the small-language goal |

## Recommendation

Adopt option C together with a built-in `array[T, N]`, in one change. Defer
option D until code shows a need. Reject E. The [decisions](#decisions) record
what is settled.

### Declaring

```text
class Ring[T, const N: i64]:
    items: array[Option[T], N]
    head: i64
    count: i64

def filled[T, const N: i64](value: T) -> array[T, N]:
    array[T, N].filled(value)
```

`const N: TypeName` declares a value parameter. The keyword is already reserved
and already means "compile-time value" at module scope. Without it, `[N: i64]`
would read like a bound, and `T: IntType` (a type) beside `N: i64` (a value)
would differ only by what the right-hand name is. One marker keeps the rule
mechanical.

Value parameter types: the eight integer types and `bool`; `u8` and `bool`
cost little once integers work. No floats (equality
and NaN make identity awkward), no `str`, no user types. Value and type
parameters may appear in any order. Functions, methods, classes and enums
accept them, as they accept type parameters; protocols do not in the first
version.

### Arguments

A value argument is one of:

| Form | Example | Notes |
| --- | --- | --- |
| Integer or `bool` literal, optionally negative | `array[u8, 64]` | Typed by the parameter's declared type, as a nongeneric parameter types its literal |
| Module constant, including an imported `pub const` | `array[u8, block_size]` | Must have exactly the parameter's type; no widening |
| A value parameter in scope, bare | `array[T, N]` | Forwarding only |

Nothing else: no `N + 1`, no calls, no type constants such as `u8.MAX`
(name a module constant instead). A derived size is named once as a module
constant, which composes with the parallel work on constant initializers:
whatever arithmetic a `const` initializer accepts is evaluated in the
declaration, not in the brackets. Inside a generic, a derived size cannot be
named; that is option D.

This rule also bounds expansion. A body can request only values that already
appear in the program: literals, constants, and its own parameters. Value
arguments cannot grow without end, so the existing type-driven limits remain the
only expansion guard needed.

### In bodies

`N` is an immutable value of its declared type. Each instance substitutes the
number, so `range(N)`, `N * 2`, `if N > 0:` and comparisons compile to
immediates, as module constant reads do. A local cannot shadow a value
parameter, matching the existing rule that a type parameter name cannot also
name a value binding.

Since bodies are checked per instance, a body rejected for one value, such as
`array[T, N]` with a negative `N`, is reported as an error of that instance,
at the use that required it, through the existing
[instance diagnostics](../design/language/07-generics/index.md#diagnostics-from-instances).

### Type identity and checking

Two instances are the same type when their declarations match and their
arguments are equal after resolving aliases and constants: with
`const width: i64 = 4`, `array[i64, width]` is `array[i64, 4]`. Instances print
with values: `array[i64, 4]`, `Flags[True]`. A type alias can name a complete
instance (`type Block = array[u8, 512]`), since aliases already name complete
generic instances.

At declaration, the checker verifies kinds: a type where a type parameter is
expected, a value of the right type where a value parameter is expected. Value
constraints such as `N > 0` are not part of the signature. A data type that
needs one states it with a [per-instance assertion](#per-instance-assertions);
`array` rejects a negative length the same way, built in.

### Per-instance assertions

The module-scope `assert COND, "message"` from local PR #628 is also accepted
in class bodies and generic function bodies:

```text
class Ring[T, const N: i64]:
    assert N > 0, "Ring needs a positive capacity"
    items: array[Option[T], N]
    head: i64
    count: i64

def best_window[const N: i64, const W: i64](xs: &array[i64, N]) -> i64:
    assert 0 < W and W <= N, "window must fit the array"
    ...
```

- **Position.** Assertions come before any other item or statement in the
  body. There `assert` begins the static form, as at module scope; elsewhere in
  a function body it stays an ordinary name, as PR #628 leaves it.
- **Operands.** The condition and message may use constants and the
  declaration's value parameters only: no locals, fields, calls or type
  parameters. The checker rejects anything else at declaration, once, not per
  instance.
- **When it runs.** Once per instance, after substitution, whenever the type or
  function is instantiated. That includes a type named only in a field,
  binding or signature with no method ever called. This matters because
  methods are checked only when used; an assertion attached to a method would
  never fire for an instance that only stores data.
- **Failure.** Reported at the use that required the instance, with a note at
  the assertion, like the [instance diagnostics](../design/language/07-generics/index.md#diagnostics-from-instances)
  added for issue #518:

```text
main.plenty:9:11: cannot instantiate `Ring[i64, 0]`: static assertion failed: Ring needs a positive capacity
  ring.plenty:2:5: note: in instantiation `Ring[i64, 0]`: assert N > 0
```

In a nongeneric class or function the assertion is accepted and runs once, like
a module assertion; it is mainly useful in generic code.

### Inference

Value parameters are inferred by the existing structural match: `array[T, N]`
against `array[i64, 4]` learns `T = i64` and `N = 4`. A literal passed where
the parameter type is an array also supplies `N` from its element count; see
[literals at generic parameters](#literals-at-generic-parameters). Nothing else
supplies evidence: not a list's length, not an ordinary argument's value. Output-only
value parameters need the explicit list, as type parameters do:
`filled[i64, 4](0)`. Partial explicit lists stay unsupported (B16).

In expressions, `filled[i64, 4](0)` keeps today's bracket rule: brackets after a
local index it, brackets after a declaration are generic arguments. The type
argument grammar gains literals, so `xs[0]` on a local still indexes.

### The first consumer: `array[T, N]`

A built-in fixed-length inline sequence. This page fixes only what value
parameters need; its complete API needs its own review.

| Property | Contract |
| --- | --- |
| Storage | `N` elements inline in the containing value, no header, no allocation |
| Copy and move | Copyable when `T` is copyable, otherwise moves, as tuples do |
| Construction | Complete: `filled(value)` for copyable `T`, a [literal](#array-literals) with an expected `array` type, or a callback per index. Never partly initialized. |
| Access | `a[i]` with a runtime index, bounds-checked like `list`, trapping out of range; `&mut a[i]` under ordinary loan rules; `len(a)` is `N` |
| Iteration | Borrowed iteration yields references, as for lists |
| Equality, printing | Structural, as for tuples |
| Size | Element size times `N`; large frames are bounded by the existing frame-size diagnostic (issue #441) |

`BoundedList[T, N]` and a `std.simd` rewrite are library code over this type.

### Array literals

A list literal `[1, 2, 3]` is typed `array[T, N]` when its expected type is an
`array` instance, after resolving aliases. It then needs no `?`:

```text
lanes: array[i32, 4] = [1, 2, 3, 4]
values: list[i32] = [1, 2, 3, 4]?
```

This is sound because an array literal allocates nothing. Its storage is the
destination itself: the local's frame slot, the field, or the argument or
return slot, as for a tuple. A list literal returns `Result` only because it
allocates a heap buffer that can fail. Element expressions keep their own
failures, so `[f()?, 2]` still propagates `f`'s error, as tuple components do.
The element count is known when parsing, so a count other than `N` is a
compile-time error, and `T` gives each element its expected type, as with
`add[u8](1, 2)`.

The expected type comes from the places that already supply one:

| Source | Example |
| --- | --- |
| Annotated binding | `xs: array[i64, 3] = [1, 2, 3]` |
| Concrete parameter type, including constructor fields and explicit generic argument lists | `checksum([1, 2, 3])` for `checksum(block: array[u8, 3])`; `sum_all[i64, 3]([1, 2, 3])` |
| Generic parameter whose declared type is an array | `sum_all([1, 2, 3])` for `sum_all[T, const N: i64](xs: array[T, N])`; see below |
| Declared return type | `return [0, 0]` in a function returning `array[i64, 2]` |

Without one, `[...]` stays a list. Comprehensions and empty `[]` are not array
literals: a comprehension's length is not static, and `array[T, 0]` is built
with `filled`.

#### Literals at generic parameters

A generic parameter still supplies no expected *type*, since `T` and `N` are
unknown until inference finishes. It does supply an expected *shape*. Parameter
types are known from the signature before any argument is typed, so the
checker can see that `xs: array[T, N]` is an array before it knows `T` or `N`:

1. For each argument, the checker reads the parameter's declared type after
   resolving aliases. If it is an `array` instance, a `[...]` literal there is
   an array literal. The shape is also visible through `&`, tuple components
   and the element type of an enclosing array, so `[[1, 2], [3, 4]]` for
   `array[array[T, N], M]` is two levels of array.
2. The literal's element count is evidence for the length argument: `N = 3`.
3. Elements are typed as any generic argument is today, without an expected
   type: unsuffixed integers default to `i64`. Their common type is evidence
   for `T`. `sum_all[u8, 3]([1, 2, 3])` or suffixed elements choose another
   type.
4. Evidence from all arguments is merged and must agree, as now. Arguments
   still evaluate once, in source order.

The shape stops at type parameters and other nominal types. A parameter typed
as bare `T` gets a list from `[1, 2, 3]`, which needs `?` as usual; so does
`Option[array[T, N]]`, whose argument is a constructor call, not a literal.

One call can mix shapes. For
`def pair[T, const N: i64](names: list[T], codes: array[T, N])`,
`pair([1, 2]?, [3, 4, 5])` builds a list and a three-element array, and both
give evidence for `T`. `pair([1u8]?, [3, 4])` fails as `add(1u8, 2)` does:
`T` is `u8` from one argument and `i64` from the other.

### Instances, limits and compile cost

Each distinct value is a distinct instance with its own code. `array[u8, 16]`
and `array[u8, 32]` duplicate every method they use. Value instances count
against the same per-template (16,384) and per-compilation limits as type
instances; no separate budget is needed, since arguments cannot grow.

Code size is the real cost. Libraries should keep length-independent work in
functions that take a borrowed view, so only thin wrappers depend on `N`. That
needs borrowed sub-range views (B11/B12); until then, the guidance belongs in
library documentation rather than the compiler.

### Interactions

| Area | Effect |
| --- | --- |
| Ownership | Value arguments are copied numbers; no loans. `array[T, N]` follows tuple ownership. |
| `--check-all-methods` | Unchanged: it checks every method of each instance the program creates; a value-generic type with no instance stays unchecked |
| Compile cache | Instances are ordinary functions with `N` folded to immediates; content keys need no change ([cache](../design/toolchain/02-architecture/cache.md)) |
| Runtime metadata | `array` needs a type descriptor carrying element descriptor and count, for drop, equality and printing |
| `std.simd` | Public shapes stay closed and named; any generic implementation is internal and must keep the native lowering's matches |
| Module constants | Constants become usable as arguments; the constant work's arithmetic and static assertions provide derived sizes and checks without bracket expressions |
| C exports | Public instances already need public concrete arguments; a value argument is a number, so it adds no visibility requirement |

### Diagnostics

```text
main.plenty:4:24: value arguments must be a literal, a module constant, or a value parameter; found `N + 1`
  note: name a derived size with a module constant: `const padded: i64 = ...`
main.plenty:6:11: `array` expects a type for `T`, found the value `4`
main.plenty:7:11: value argument for `N` must be `i64`, found `u8` constant `width`
main.plenty:9:5: cannot infer `N` for `filled`; supply every argument: `filled[i64, 4](0)`
main.plenty:12:9: `array[i64, -1]`: an array length cannot be negative
main.plenty:14:12: conflicting evidence for `N` in `zip_add`: 4 from argument 1 (`array[i64, 4]`), 3 from the array literal in argument 2
main.plenty:15:10: conflicting evidence for `T` in `pair`: `u8` from argument 1, `i64` from the array literal in argument 2
  note: give the literal's elements a type, such as `3u8`, or supply every argument: `pair[u8, 2](...)`
main.plenty:16:14: `?` needs a `Result`, found `array[i64, 3]`
  note: a literal passed as `array[T, N]` is an array, which allocates nothing and cannot fail
main.plenty:17:9: cannot infer `T` for `sum_all` from an empty literal; `[]` is not an array literal
  note: build an empty array with `filled`, or supply every argument
main.plenty:18:9: `sum_all` expects `array[T, N]` for `xs`, found `list[i64]`
  note: only a literal becomes an array here; a list does not convert
```

## Rejected alternatives

| Alternative | Reason |
| --- | --- |
| `[N: i64]` without `const` | Confuses bounds with value types; learners need one visible marker |
| Expressions in brackets (Rust `generic_const_exprs`) | Unification of expressions, evaluatability bounds, and unbounded instance growth |
| Zig-style compile-time functions | A second evaluation model; against small-language and fast-compile goals |
| Float, `str` or user-type values | Identity and printing questions for no current use; `str` can be reconsidered after string constants land |
| Inferring `N` from runtime values or list lengths | Hidden specialization per call site and no static guarantee |
| Lane-count-generic public `std.simd` | Already rejected by the [SIMD API](std-simd/api.md) |
| Value parameters without an array type | Saves a field; no motivating use |

## Decisions

1. **Spelling: `[T, const N: i64]`.** Rejected: `[T, N: i64]`, which reads
   like a bound.
2. **Value types: the eight integer types and `bool`.** Rejected: `i64` only,
   and floats, `str` and user types.
3. **Name and literal: lowercase `array[T, N]`,** matching `list` and `tuple`.
   A literal with an expected `array` type builds one inline without `?`; see
   [array literals](#array-literals).
4. **Value parameters and `array` ship together.** Rejected: parameters first
   for phantom-style uses, which have no motivating case.
5. **Derived sizes (option D): deferred.** Add expressions such as
   `array[T, N + 1]` in signatures only with evidence from `BoundedList` or
   `std.simd` code.

6. **Per-instance checks: `assert` in class and generic function bodies.** The
   module-scope `assert` from PR #628 runs once per instance, from constants
   and value parameters, whenever the type or function is instantiated, with
   failures reported at the instantiation site; see
   [per-instance assertions](#per-instance-assertions). Rejected: each type
   validating its own arguments, which only built-in types such as `array` can
   do at compile time; a user type would have to check in its constructor and
   fail at run time.
7. **Array literals at generic parameters.** A literal where the parameter's
   declared type is an array builds an array and supplies `N`, so
   `sum_all([1, 2, 3])` needs no argument list; see
   [literals at generic parameters](#literals-at-generic-parameters). Rejected:
   requiring `sum_all[i64, 3](...)`, which makes the common call verbose for no
   gain in clarity.

## Implementation sketch

1. **Parameters.** Parse `const N: T` in parameter lists and literals in type
   argument lists. Represent a generic argument as a type or a value; a
   separate argument kind is cleaner than a `Ty` leaf that every type position
   must reject, but touches every place that stores instance arguments as
   `Vec<Ty>`. Extend `Substitution` with value entries, substitute `N` in
   bodies as a literal, extend instance names and structural inference. Tests:
   kinds, constants as arguments, identity across aliases and constants,
   inference, rejected expressions, phantom value tags.
2. **Per-instance assertions.** After PR #628 merges: accept `assert` at the
   start of class and function bodies, check at declaration that operands are
   constants and value parameters, and evaluate after substitution whenever an
   instance is created, including data-only instances whose methods are never
   checked. Failures use the instance diagnostic chain. Tests: failing and
   passing instances, a type used only as a field, rejected operands, `assert`
   as an ordinary name later in a body.
3. **`array[T, N]`.** Inline layout, descriptor, construction including
   literals with an expected type or at array-typed generic parameters, checked
   indexing, borrowed iteration, equality, printing, drop of affine elements.
   Reference page, tutorial lesson in the generics chapter, cheat sheet entry.
4. **Library use.** `BoundedList[T, N]` under B13. Measure an internal
   `std.simd` rewrite against the current code: native lowering must still
   recognize the shapes, and object code must not regress.
5. **Deferred: derived sizes.** Only if stage 4 code shows a need: expressions
   in non-inferred output positions, evaluated after substitution.
