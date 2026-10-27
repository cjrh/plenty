# Value generic parameters

**Status: Not implemented.** Nothing shipped. Generic parameters are types only;
the [generics reference](../design/language/07-generics/index.md) is
authoritative. No backlog entry exists yet; the closest rows are B13 (bounded
and inline-capacity storage) and B23 (SIMD). Researched against local `main` at
`1b5b96b1` on 2026-10-27.

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
Inline aggregates now exist. This page designs the constant parameters.

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
| `std.simd` lane types | Hand-written payload fields per shape; native lowering matches six names (`src/codegen/portable_simd.rs`) | An internal `Lanes[T, const N: i64]` over `Array[T, N]` could share bodies, with the public names kept as aliases. Lowering would match instance identities. The [SIMD API](std-simd/api.md) keeps a closed public set; that decision stands. |
| Bounded containers (B13) | Not expressible without allocation | `BoundedList[T, const N: i64]` over inline storage reports capacity exhaustion without allocating |
| `std.archive` 512-byte tar blocks (`block_size`) | Heap `Bytes` | `Array[u8, block_size]` once reads can target a borrowed byte view (B11/B39) |
| `std.random` `state() -> (u64, u64, u64, u64)` | Tuple | No real gain: tuples cover small fixed groups in non-generic code |
| `std.sketch` `HyperLogLog` precision | Runtime field, validated, checked on merge, read from serialized data | Worse: `from_bytes` reads the precision from data, so it must stay a runtime value |
| `std.crypto` digest size | Runtime `Algorithm`; `finish_into` writes into a caller buffer without allocating | No gain: the algorithm is a runtime choice and the allocation-free path exists |
| `clist` chunks | Physical storage kept private so chunk size can change ([chunks](columnar-list/chunks.md)) | Not wanted: a public chunk-size parameter would fix what the design keeps private |

Every case that benefits needs indexable inline storage. Value parameters
without an array type would only save a field. They should ship with
`Array[T, N]`, not before it.

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
| C. Minimal value parameters: integer and `bool` values, bare arguments only | Covers arrays, bounded containers and fixed lanes; no expression unification; finite instance set | No `Array[T, N + 1]` in signatures |
| D. C++-style: expressions in non-inferred type positions, evaluated per instance | `concat` and `push` can name result sizes | Errors appear at the use, inside the callee's signature; more checking machinery |
| E. Zig-style compile-time evaluation | Most expressive | A compile-time interpreter, evaluation budgets, and generic code that is hard to read; against the small-language goal |

## Recommendation

Adopt option C together with a built-in `Array[T, N]`. Keep option D as a
possible later stage that needs evidence. Reject E.

### Declaring

```text
class Ring[T, const N: i64]:
    items: Array[Option[T], N]
    head: i64
    count: i64

def filled[T, const N: i64](value: T) -> Array[T, N]:
    Array[T, N].filled(value)
```

`const N: TypeName` declares a value parameter. The keyword is already reserved
and already means "compile-time value" at module scope. Without it, `[N: i64]`
would read like a bound, and `T: IntType` (a type) beside `N: i64` (a value)
would differ only by what the right-hand name is. One marker keeps the rule
mechanical.

Value parameter types: the eight integer types and `bool`. No floats (equality
and NaN make identity awkward), no `str`, no user types. Value and type
parameters may appear in any order. Functions, methods, classes and enums
accept them, as they accept type parameters; protocols do not in the first
version.

### Arguments

A value argument is one of:

| Form | Example | Notes |
| --- | --- | --- |
| Integer or `bool` literal, optionally negative | `Array[u8, 64]` | Typed by the parameter's declared type, as a nongeneric parameter types its literal |
| Module constant, including an imported `pub const` | `Array[u8, block_size]` | Must have exactly the parameter's type; no widening |
| A value parameter in scope, bare | `Array[T, N]` | Forwarding only |

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
`Array[T, N]` with a negative `N`, is reported as an error of that instance,
at the use that required it, through the existing
[instance diagnostics](../design/language/07-generics/index.md#diagnostics-from-instances).

### Type identity and checking

Two instances are the same type when their declarations match and their
arguments are equal after resolving aliases and constants: with
`const width: i64 = 4`, `Array[i64, width]` is `Array[i64, 4]`. Instances print
with values: `Array[i64, 4]`, `Flags[True]`. A type alias can name a complete
instance (`type Block = Array[u8, 512]`), since aliases already name complete
generic instances.

At declaration, the checker verifies kinds: a type where a type parameter is
expected, a value of the right type where a value parameter is expected. Value
constraints such as `N > 0` are not part of the signature. A data type that
needs one rejects bad values when its instance is first required; `Array`
rejects a negative length this way. A general per-instance assertion form
should reuse whatever module-level static assertion the constant work adds,
rather than invent a `where` clause.

### Inference

Value parameters are inferred by the existing structural match: `Array[T, N]`
against `Array[i64, 4]` learns `T = i64` and `N = 4`. Nothing else supplies
evidence: not a list's length, not an ordinary argument's value. Output-only
value parameters need the explicit list, as type parameters do:
`filled[i64, 4](0)`. Partial explicit lists stay unsupported (B16).

In expressions, `filled[i64, 4](0)` keeps today's bracket rule: brackets after a
local index it, brackets after a declaration are generic arguments. The type
argument grammar gains literals, so `xs[0]` on a local still indexes.

### The first consumer: `Array[T, N]`

A built-in fixed-length inline sequence. This page fixes only what value
parameters need; its complete API needs its own review.

| Property | Contract |
| --- | --- |
| Storage | `N` elements inline in the containing value, no header, no allocation |
| Copy and move | Copyable when `T` is copyable, otherwise moves, as tuples do |
| Construction | Complete: `filled(value)` for copyable `T`, a literal with an expected `Array` type, or a callback per index. Never partly initialized. |
| Access | `a[i]` with a runtime index, bounds-checked like `list`, trapping out of range; `&mut a[i]` under ordinary loan rules; `len(a)` is `N` |
| Iteration | Borrowed iteration yields references, as for lists |
| Equality, printing | Structural, as for tuples |
| Size | Element size times `N`; large frames are bounded by the existing frame-size diagnostic (issue #441) |

`BoundedList[T, N]` and a `std.simd` rewrite are library code over this type.

### Instances, limits and compile cost

Each distinct value is a distinct instance with its own code. `Array[u8, 16]`
and `Array[u8, 32]` duplicate every method they use. Value instances count
against the same per-template (16,384) and per-compilation limits as type
instances; no separate budget is needed, since arguments cannot grow.

Code size is the real cost. Libraries should keep length-independent work in
functions that take a borrowed view, so only thin wrappers depend on `N`. That
needs borrowed sub-range views (B11/B12); until then, the guidance belongs in
library documentation rather than the compiler.

### Interactions

| Area | Effect |
| --- | --- |
| Ownership | Value arguments are copied numbers; no loans. `Array[T, N]` follows tuple ownership. |
| `--check-all-methods` | Unchanged: it checks every method of each instance the program creates; a value-generic type with no instance stays unchecked |
| Compile cache | Instances are ordinary functions with `N` folded to immediates; content keys need no change ([cache](../design/toolchain/02-architecture/cache.md)) |
| Runtime metadata | `Array` needs a type descriptor carrying element descriptor and count, for drop, equality and printing |
| `std.simd` | Public shapes stay closed and named; any generic implementation is internal and must keep the native lowering's matches |
| Module constants | Constants become usable as arguments; the constant work's arithmetic and static assertions provide derived sizes and checks without bracket expressions |
| C exports | Public instances already need public concrete arguments; a value argument is a number, so it adds no visibility requirement |

### Diagnostics

```text
main.plenty:4:24: value arguments must be a literal, a module constant, or a value parameter; found `N + 1`
  note: name a derived size with a module constant: `const padded: i64 = ...`
main.plenty:6:11: `Array` expects a type for `T`, found the value `4`
main.plenty:7:11: value argument for `N` must be `i64`, found `u8` constant `width`
main.plenty:9:5: cannot infer `N` for `filled`; supply every argument: `filled[i64, 4](0)`
main.plenty:12:9: `Array[i64, -1]`: an array length cannot be negative
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

## Decisions to make

1. **Spelling.** `[T, const N: i64]` (recommended) or `[T, N: i64]`.
2. **Value types.** Integers and `bool` (recommended), or `i64` only. `i64`
   only is simpler; `u8` and `bool` cost little once integers work.
3. **Array name and literal.** `Array[T, N]` or lowercase `array[T, N]`,
   matching `list` and `tuple`; and whether `[1, 2, 3]` with an expected
   `Array` type constructs one without `?`.
4. **Ship together.** Value parameters and `Array` in one change (recommended),
   or parameters first for phantom-style uses.
5. **Derived sizes (option D).** Never, or later with evidence from
   `BoundedList`/`std.simd` code.
6. **Per-instance assertions.** Reuse the constant work's static assertion
   inside generic bodies, or rely on data types validating their own arguments.

## Implementation sketch

1. **Parameters.** Parse `const N: T` in parameter lists and literals in type
   argument lists. Represent a generic argument as a type or a value; a
   separate argument kind is cleaner than a `Ty` leaf that every type position
   must reject, but touches every place that stores instance arguments as
   `Vec<Ty>`. Extend `Substitution` with value entries, substitute `N` in
   bodies as a literal, extend instance names and structural inference. Tests:
   kinds, constants as arguments, identity across aliases and constants,
   inference, rejected expressions, phantom value tags.
2. **`Array[T, N]`.** Inline layout, descriptor, construction, checked
   indexing, borrowed iteration, equality, printing, drop of affine elements.
   Reference page, tutorial lesson in the generics chapter, cheat sheet entry.
3. **Library use.** `BoundedList[T, N]` under B13. Measure an internal
   `std.simd` rewrite against the current code: native lowering must still
   recognize the shapes, and object code must not regress.
4. **Optional derived sizes.** Only if stage 3 code shows a need: expressions
   in non-inferred output positions, evaluated after substitution.
