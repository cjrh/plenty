# Value-dependent types for check elimination

**Status: Not implemented.** No part of this design has shipped. The
[`Bounded` reference](../design/language/03-types/bounded.md) and
[value parameters](../design/language/07-generics/values.md) own current
behavior. Researched against local `main` at `9daa4a78` on 2026-11-11 by
reading source, references, kernels and the issue register; nothing was
compiled or measured for this page. Instruction counts are quoted from the
kernel READMEs and issues cited beside them. Claims about current behavior are
marked **verified** with their source; everything else is proposed.

## Question

Bounds of `Bounded[T, lo, hi]` must be literals or module constants
([value generics, option C](value-generics.md#recommendation)). A bound that is
a runtime value makes a type depend on a value: a dependent type. This page
asks which checks such types would remove that the compiler does not already
remove, what each form of dependency costs, and which form fits Plenty.

Short answer: full dependent types do not fit. Most of the measurable value
comes from constant bounds, which are already accepted work (issues #937 and
#940). The one runtime-value dependency worth designing is an index type tied
to an immutable slice-view binding, `Index[v]`. It needs no solver and no new
borrow rules, because a view's length already never changes. It removes checks
that no flow analysis can: indices stored in data, returned from functions, or
passed to calls that are not inlined. Build it only after a measured kernel
shows the gain survives in a whole program.

## What the compiler already proves

These facts are delivered and verified. A proposal must not count their
gains again.

| Fact | Where | Source |
| --- | --- | --- |
| `for x in xs`, `for i in range(len(xs))`, `enumerate(xs)` over a stable list or view: no bounds check, no counter overflow check | Range counters against stable lengths | [lists](../design/facilities/01-builtins/lists.md), `Bounds` in `src/codegen/induction.rs`; issue #728 |
| `while i < n: ... i += 1`: the step needs no overflow check; a nonnegative counter uses unsigned division | Induction facts | `Facts` in `src/codegen/induction.rs`; issue #369, local PR #445 |
| `while lo < hi` reversal over a view: one check before the loop | Relational loop facts | [lists](../design/facilities/01-builtins/lists.md); local PR #932 |
| Range and `while` loops with unrelated stops: compiled twice, one test before the loop selects the unchecked copy (body up to 256 operations, no calls, no nested range loop) | Loop versioning | `Version` in `src/codegen/induction.rs`; issues #728, #766 |
| Loop-invariant and nonnegative constant indices (`perm[0]`): one test before the loop | Loop versioning | issue #766 |
| `u8`/`u16`-widened index into a table of at least 256 or 65,536 elements: one test selects the unchecked copy | Versioning on value extent | [lists](../design/facilities/01-builtins/lists.md); issue #760 |
| Signed `%` by a positive constant `M`: result in `0..M-1`, recorded as nonnegative and at most `M - 1` | Value bounds | `note_remainder` in `src/codegen.rs`; issue #937 partial fix |
| `//`, `%` by `NonZero[T]` or `Positive[T]`: no zero check; `Positive` also no `MIN // -1` check | Domain types | [numbers](../design/language/03-types/numbers.md#positive-and-non-negative-integers); issue #664 |
| Literal or module-constant divisors and shift counts: impossible errors removed from the signature and the code | Lexical proofs | issues #796, #863 |
| Small fixed numerical loops: runtime guards admit a scalar-replaced fast copy | `fixed_state` pass | `src/codegen/fixed_state.rs`; issue #930 |

Two deliberate limits are also verified. Guards do not narrow: `if d != 0:`
proves nothing about `x // d`, and `-x` inside `if x != T.MIN:` still adds
`OverflowError[T]` ([numbers](../design/language/03-types/numbers.md); issue
#796 chose a predictable lexical rule). And `unchecked:` under
`--unchecked assume` already removes arithmetic and index checks with
undefined behavior on violation. Nothing on this
page adds to that escape hatch; the aim is the same code with a proof instead
of an assumption.

> **Superseded 2026-11-12:** the `--unchecked` build mode and the
> `unchecked trap:` spelling were removed. Every `unchecked:` suite traps on a
> violation. See the
> [numeric reference](../design/language/03-types/numbers.md#unchecked-blocks).

## Remaining checks, case by case

| Case | Check left today | Would a runtime-value bound remove it? |
| --- | --- | --- |
| fasta `Rng.seed = (seed * IA + IC) % IM`, stored in a field (issue #937), and the same recurrence on a loop-carried local (issue #940) | Two overflow checks and a floor fixup, 8 of about 24 instructions per number (`experiments/kernels/field-rng/`) | No dependency needed. The bound `IM` is a module constant: `seed: Bounded[i64, 0, IM_LAST]` plus the accepted infallible `Bounded.from(e % IM)` covers it. |
| A generator with a runtime modulus `m` | Same overflow checks | No. `seed < m` with unknown `m` gives no magnitude, so `seed * a + c` can still overflow. Only a constant (or a constant cap on `m`) removes overflow checks. Nonnegativity, which `NonNegative` already gives, removes the fixup. |
| Divisor proven nonzero by a guard | Zero check | No. `NonZero.try_from(d)` already carries the proof; only a decision to narrow on guards would change this, and issue #796 rejected that. |
| Random index `xs[rng.index(len(xs))?]` (`std.random`, verified signature `index(count: i64) -> Result[i64, RandomError]`) | One bounds check and negative-index normalization per access | Yes, with an index type tied to `xs`. Small gain outside loops. |
| Indirect gather `xs[indices[i]]` (`experiments/kernels/unchecked-index/`) | Bounds check and negative-index join on the gathered index: 12 instructions per pass versus 9 under `--unchecked assume` and 5 in C | Yes. Indices validated once when built, used many times. No flow analysis can prove a value read from memory. This is the strongest case. |
| fasta `codes[k]` after `while limits[k] < r: k += 1` | Check of `codes[k]`; `limits[k]` check (2 of 8 instructions per search pass) | `codes[k]` only with a length relation `len(codes) == len(limits)`. `limits[k]` never: termination depends on the data (the last limit is `IM`). |
| n-body `dx[k]` with a triangular counter `k` over pairs `i < j < n` | Checks in the pair loops (issue #930) | No. `k < n(n-1)/2` is nonlinear arithmetic. The `fixed_state` guards already handle the benchmark's original source. |
| fannkuch `perm[k]` with `k = perm[0]` | One test per inner loop after issue #766 | Only by typing a permutation's contents as indices of itself; a per-flip test remains small. Not worth a type. |
| `for i in range(len(src)): dst[i] = src[i]` | One test before the loop plus a second loop copy | A relation `len(dst) >= len(src)` in the signature would drop the duplicate copy (code size, compile time), not per-pass work. |
| A helper `at(v, i)` that is not inlined, called with a proven `i` | The helper rechecks | Yes. The fact crosses the call only if it is in the signature. |
| k-nucleotide `to_text_range(i, i + length)` | Clamps already skipped for key construction (`experiments/key-construction/`) | Little; hashing dominates. Not measured. |
| `std.graph` node and edge identifiers into internal lists | Checks at each lookup | No. The lists grow and shrink; their identifiers need a container invariant, the subject of [checked node identities](completed/arenas.md). |

The pattern: per-iteration checks inside loops are mostly gone. What remains
splits into (a) magnitude facts about stored or loop-carried values, which
constant bounds solve; (b) data-dependent invariants (search termination,
nonlinear counters), which no tractable type system proves; and (c) indices
that leave the loop that created them: stored, returned, or passed across a
call. Value-dependent index types address only (c).

Issue #728 recorded that bounded indices "showed no measured gain" in the
kernels then studied. Those kernels were (a) and (b) cases. Category (c) has
one kernel measurement (the gather) and no whole-program one.

## Design space

| Option | Removes | Compile cost | Diagnostics | Learner burden |
| --- | --- | --- | --- | --- |
| A. Constant bounds (accepted) and closed constant expressions | Overflow and sign checks on stored and loop-carried values | Constant folding only | Local and exact | Low: one more argument form |
| B. Bounds naming an immutable value: `Index[v]`, `Bounded[i64, 0, n]` | Bounds checks on stored, returned and passed indices | Name comparison per use | "index of `w` used for `v`" | Moderate: one new kind of type argument |
| C. Branded/generative indices (Rust `indexing`, GhostCell) | Same as B | Same as B | Rust's are lifetime errors; poor | High in Rust; in Plenty, B already is the brand |
| D. Refinement types with a solver (Liquid Haskell, Flux, F\*, SPARK) | Most of B plus relations and some invariants | Solver calls per function; seconds for hard functions | "could not prove `k < len(dx)`" with no fix suggested; varies by solver version | High: annotations, invariants, solver habits |
| E. Full dependent types (Idris, Lean, ATS) | Anything provable, including permutations | Type checking evaluates programs; termination checking | Proof-state errors | Very high |

### A. Constant bounds

Accepted: issues #937 and #940 make code generation use a `Bounded` value's
range, and add infallible `Bounded[T, lo, hi].from(e)` that compiles only when
a fixed rule proves `e` in range, the first rule being `e % c` with constant
`c > 0`. This covers fasta's RNG and any recurrence modulo a constant.

Two small extensions:

- **Closed constant expressions as bounds.** `Bounded[i64, 0, IM - 1]` instead
  of naming `const IM_LAST: i64 = IM - 1`. An expression over literals and
  module constants only, with no value parameter, evaluates to a number when
  the type is resolved, so identity stays a comparison of numbers and none of
  the unification problems that deferred
  [derived sizes](value-generics.md#decisions) apply. Cost: the constant
  evaluator already exists for `const` initializers. Benefit: cosmetic; a named
  constant works today.
- **Half-open bounds.** A second spelling (`Below[T, n]` for `0 <= v < n`)
  would match `range` and `len`, but two spellings for one idea contradicts
  the preference for one rule. Recommendation: keep `Bounded` inclusive only;
  closed constant expressions make `n - 1` cheap to write.

### B. Bounds that name an immutable value

A type argument may name an immutable binding in scope, and the type's meaning
depends on that binding's value. Two forms:

- `Bounded[i64, 0, n]` with `n` an immutable integer binding. It says nothing
  about magnitude, so it removes no overflow checks (see the table above).
  It removes a bounds check only if the compiler also knows some list has
  length greater than `n`, which needs length relations. Alone it buys little.
- `Index[v]` with `v` an immutable binding of a slice view, `&[T]` or
  `&mut [T]`. It means `0 <= i < len(v)`. `v[i]` needs no check and no
  negative-index normalization. This is the form proposed
  [below](#proposed-design-indexv-over-view-bindings).

Why views, and why Plenty is well placed: a view's length never changes
(**verified**, [slices](../design/language/04-expressions/slices.md#slice-views)),
and an immutable binding always names the same view. So `len(v)` is one fixed
number for the whole scope of `v`, and an `Index[v]` value stays valid for
exactly as long as `v` can be used. The existing loan on the viewed list
already stops it being resized, moved or replaced while `v` is live. No new
loan kind, no lifetime parameter, and no "length loan" is needed. Element
writes through `&mut [T]` remain allowed, because they cannot change the
length.

An owned `list` or a `&mut list[T]` can resize, so it cannot be named. A
program takes a view first: `v = &xs[:]`.

### C. Branded indices

Rust's `indexing` crate (bluss) and GhostCell (Yanovski et al., ICFP 2021)
create a fresh invariant lifetime `'id` inside a closure, so `Index<'id>`
values fit only the container branded `'id`. The brand is a type-level name
for one container. Rust needs the closure and lifetime trick because Rust
values have no stable names in types. In Plenty the immutable binding *is* the
brand, so option B is option C without the encoding. Adopt the semantics,
not the encoding.

### D. Refinement types with a decision procedure

Liquid Haskell and Flux attach predicates (`{v: usize | v < n}`) and discharge
them with an SMT solver (Z3 or similar) over Horn constraints; F\*, Dafny and
SPARK (GNATprove) do the same with verification conditions. They prove
relations such as `len(codes) == len(limits)` and loop invariants with
annotations.

Against Plenty's goals:

- **Compile time.** A solver call per verification condition; published
  experience with Dafny and SPARK reports functions that take seconds and
  proofs that become slow or fail after unrelated changes. Plenty measures
  compile time per function and caches compiled functions; a nondeterministic
  or version-dependent proof step does not fit either.
- **Dependency.** Z3 or cvc5 is a large C++ dependency. The toolchain embeds
  its runtime and standard library and avoids such dependencies.
- **Diagnostics.** A failed proof reports what could not be proven, not what
  to change. Learners cannot predict which program the solver accepts.
- **Rules.** "Whatever the solver proves" is not a hard, exception-free rule.

A fixed decision procedure for difference constraints (`x - y <= c`, as in
ABCD, Bodík, Gupta and Sarkar, PLDI 2000, and in Dependent ML, Xi and Pfenning,
PLDI 1998) is cheap and predictable. Plenty's code generator already runs a
version of it internally (`Bounds`, `below`, invariant locals). Moving it into
the type checker would make its limits part of the language contract; the
code generator can extend it without that commitment. Reject D for the type
system.

### E. Full dependent types

Idris (`Fin n`, `Vect n a`), Lean and ATS can state and prove a permutation's
contents or n-body's pair count. The price is a type checker that evaluates
terms, termination checking for anything used in types, user-written proofs,
and erasure rules (Idris 2's quantities). Each is a large subsystem, and the
learner must understand all of them to read an error. This contradicts a
small language with fast compilation. ATS shows the performance is real and
also how few programmers write such code. Reject E.

### Prior art

| System | Mechanism | Lesson for Plenty |
| --- | --- | --- |
| Wuffs | Bounds and overflow proven at compile time, no runtime checks. Refinement types such as `base.u8[..= 15]`, interval arithmetic, facts from assignments and `if`/`while` conditions, loop `pre`/`inv`/`post` clauses, `assert` facts. No SMT solver; a failed proof is a compile error. | Closest model for option A: fixed interval rules, compile error on failure. Plenty keeps runtime checks as the default rather than requiring every index to be proven. |
| Ada / SPARK | Range subtypes, including dynamic bounds (`subtype Index is Integer range 1 .. N` with runtime `N`); discriminated records size arrays by a field. Checks at conversion; GNATprove proves absence of runtime errors with solvers. | Dynamic subtypes are checked at entry, then trusted: the shape of `Index[v].try_from`. SPARK shows the solver cost. |
| Dependent ML, ATS | Integer indices in types, linear constraints solved by a built-in procedure (ATS can call Z3). | Restricted index domains keep checking decidable, but signatures fill with index variables. |
| Liquid Haskell, Flux | Refinement predicates, SMT, inference of refinements by predicate abstraction. | Expressive; solver cost and unpredictability. |
| Idris, Lean | Full dependent types; `Fin n` indices. | Too large for Plenty. |
| Dafny, F\* | Verification conditions to SMT. | Annotation burden and proof brittleness. |
| Rust | Const generics (bare arguments in stable); `generic_const_exprs` unstable for years; `indexing` crate brands; the optimizer removes checks it can prove. | Bare-argument restriction works; expressions in types are hard; brands work but read poorly. |
| C++ | `std::span<T, N>` static extents; C++26 contracts may be ignored by build mode. | Static extents match option A; contracts that can be turned off are an unchecked escape hatch. |
| Zig | `comptime` values may parameterize types; runtime values may not. Safety checks are build-mode dependent. | Confirms the split between compile-time and runtime values. |
| Go, HotSpot | Compiler-only bounds-check elimination (Go's prove pass; the `_ = s[n-1]` hint idiom). | What Plenty's code generator already does; a check hoisted by hand is a weaker `Index` with no type. |

## Proposed design: `Index[v]` over view bindings

All syntax here is proposed.

### The type

`Index[v]` is an integer in `0 <= i < len(v)`, stored as `i64`. `v` must be an
immutable binding, a parameter or local, whose type is `&[T]` or `&mut [T]`.
The type is valid only in the scope of `v`. It is copyable, adds no fields and
erases to `i64` before code generation, as `Quantity[T, D]` erases to its
storage type ([quantities](../design/language/03-types/quantities.md)).

```text
def gather(xs: &[f64], indices: &[Index[xs]]) -> f64:
    mut total = 0.0
    for i in indices:
        total += xs[*i]          # no check: *i is an Index[xs]
    total
```

A later parameter's type may name an earlier view parameter, as a value
parameter's type may already name a preceding `T: IntType`
(**verified**, [value parameters](../design/language/07-generics/values.md)).

### Introduction: the only ways to get an `Index[v]`

Each form checks once or needs no check. The list is closed; there is no
assertion form and no unchecked constructor.

| Form | Check | Result |
| --- | --- | --- |
| `Index[v].try_from(i)` | `0 <= i < len(v)`, once | `Result[Index[v], RangeError]` |
| `v.indices()` | None | A range-like generator of `Index[v]` |
| `Index[v].check_all(raw)` with `raw: &[i64]` | One pass over `raw` | `Result[&[Index[v]], RangeError]`, a view of the same memory, no copy |
| A function returning `Index[p]` or `Option[Index[p]]` for its view parameter `p` | Inside that function | `Index[w]` for the caller's argument `w` |
| `i.next()`, `i.prev()` | One comparison | `Option[Index[v]]` |
| A literal where `Index[v]` is expected | Rejected: `len(v)` is not known at compile time | — |

`v.indices()` is new rather than retyping `for i in range(len(v))`, so
existing loops keep `i: i64` and their arithmetic. Existing APIs such as
`v.find(x) -> Option[i64]` keep their types; a typed variant is an
[open question](#open-questions).

Arithmetic needs `i.get()`, which yields `i64`, as with `Bounded`. Results go
back through `try_from`. There are no arithmetic rules on `Index`.

### Elimination

| Use | Effect |
| --- | --- |
| `v[i]`, `v[i] = x`, `&v[i]`, `&mut v[i]`, augmented assignment | No bounds check, no negative-index normalization |
| `v.swap(i, j)` with both indices `Index[v]` | No checks |
| `w[i]` for another view `w` | Rejected; convert with `Index[w].try_from(i.get())` |
| `i.get()` | The `i64` |

### Passing and returning

A parameter or return type that names a view parameter is a dependent
signature. At a call, the argument for that view parameter must be a bare
name of an immutable view binding, so the result type can name it:

```text
def argmax(v: &[f64]) -> Option[Index[v]]:
    ...

def main() -> Result[(), AllocError | IoError]:
    xs = [3.0, 9.0, 4.0]?
    v = &xs[:]
    match argmax(v):
        case Some(i):
            print(v[i])?      # no check
        case Nothing:
            pass
    Ok(())
```

`argmax(&xs[:])` is rejected with a note to bind the view first, matching the
existing advice to bind a call result before binding a view of it
([slices](../design/language/04-expressions/slices.md#slice-views)).
The rule is mechanical: dependent arguments are bare names, as value
arguments are bare literals and constants under
[option C](value-generics.md#arguments).

### Failed proofs

A failed proof is a compile error that names the conversion to use. There is
never a silent runtime check and never an unchecked path:

```text
main.plenty:7:15: `i` is `i64`, but `v[...]` here expects `Index[v]` from `gather`'s signature
  note: convert once with `Index[v].try_from(i)?`
main.plenty:9:11: `Index[v]` used to index `w`
  note: an index of one view does not fit another; use `Index[w].try_from(i.get())`
main.plenty:11:9: `Index[xs]` needs an immutable view binding; `xs` is `list[f64]`
  note: bind a view: `v = &xs[:]`
main.plenty:13:5: `j: Index[v]` cannot outlive `v`
main.plenty:15:20: dependent argument must be a name; found `&xs[:]`
```

Ordinary indexing with `i64` is unchanged: it checks at run time, as today.
`Index` is an opt-in that moves the check to where the index is created.

### Rules without a solver

The checker needs only:

1. Name resolution: which binding a type argument names, and that it is
   immutable with a view type.
2. Scope: a type naming `v` may not appear outside `v`'s scope, including in
   a returned type, a field of a class or an escaping closure.
3. Substitution at calls: parameter names in a signature replaced by the bare
   argument names.
4. Equality of bindings for `Index[v] == Index[w]`.

No arithmetic, no flow facts, no fixpoint. Flow narrowing (`if i < len(v):`
making `i` an `Index[v]` in the branch) is deliberately not proposed: it would
reverse the lexical rule chosen in issue #796 for divisors, and one explicit
`try_from` costs the same single check.

## Interactions

| Area | Effect |
| --- | --- |
| Value generics, option C | Unchanged for value parameters. Dependent arguments are a separate kind (a binding), only in `Index`, only bare. Closed constant expressions (option A) are an independent extension. |
| Generics and instances | `Index[v]` erases to `i64` before monomorphization, so `list[Index[v]]` and `list[Index[w]]` share `list[i64]`'s instance; branding adds no instances. A type parameter may bind to `Index[v]` within `v`'s scope. |
| `array[T, N]` (B83) | Its index type would be `Bounded[i64, 0, N - 1]`, a derived size that option C defers. A built-in index type for arrays is an [open question](#open-questions). |
| Borrowed views (issue #945) | Step 1 (returning views) makes `Index[r]` for a returned view `r` useful. Step 2 (view classes) would want fields such as `cursor: Index[self.data]`, a field-path brand; defer until #945 lands. A matrix view whose constructor checks `rows * cols <= len(data)` once is the same idea for fields. |
| Mutation and aliasing | Element writes through `&mut [T]` keep indices valid. The viewed list's existing loan forbids resize while `v` is live. A `mut` view binding cannot be named, so a brand never refers to two views. |
| Closures | Non-escaping closures may capture `Index[v]` values when they also may use `v`. Stored closures may not capture branded values in the first version (scope rule). |
| Threads | Workers are scoped and keep a borrowed argument's loan until join (**verified**, [`std.thread`](../design/stdlib/thread.md)), so a worker may receive `v` with its indices. An owned job that outlives the call cannot hold a view, so it cannot hold an `Index[v]`. |
| Separate compilation | Signatures carry all facts; checking a call reads only the callee's signature. No whole-program analysis. |
| Compile cache | Brands are erased; omitted checks appear in the IR stencil, which is already the key ([cache](../design/toolchain/02-architecture/cache.md)). No key change. |
| `unchecked:` | Unaffected. `Index` gives assume-mode code under the default trap mode, safely. |
| `Bounded`, `NonNegative` | Independent. `Index[v]` implies nonnegative; `.get()` should carry that fact into arithmetic. |

## What the code generator would omit, and how to measure it

| Mechanism | Omitted | Kernel | Expected |
| --- | --- | --- | --- |
| A: `Bounded` constant facts and `from` (accepted) | Overflow checks and floor fixup on the seed | `experiments/kernels/field-rng/` `fill`, `fill_local` | About 8 instructions per number, from the issue #937/#940 attribution |
| B: `Index[v]` stored indices | Bounds check and negative-index join | `experiments/kernels/unchecked-index/` `gather` | 12 to 9 instructions per pass: the assume-mode result, now under trap mode |
| B: `Index[v]` across a non-inlined call | The callee's check | New kernel needed | Unknown |
| B: length relation in signatures (deferred) | Second versioned loop copy | Copy loops | Code size and compile time, not CPU |

Measurement follows the [emitted-code procedure](../development/emitted-code.md):

1. Before compiler work, hand-edit the dumped CLIF to the code `Index` would
   produce. For the gather, `assumed.clif` and `lowered.clif` in
   `experiments/kernels/unchecked-index/` already are that code.
2. Find a whole program where (c)-category checks matter: a sparse
   matrix-vector product over CSR column indices, permutation application, or
   a lookup through a validated index table. Count instructions per pass
   against C and Rust as supporting evidence.
3. Decide on CPU time of the whole program with `benchmarks/game/bench.py`
   style repetitions, pinned to physical cores. Instruction counts explain;
   CPU time decides.
4. Measure compile time with the `compile_bench` AOT workload; the checker
   change should be within noise.

## Recommendation

1. **Finish option A first (accepted).** Issues #937 and #940: code generation
   uses `Bounded` ranges, and `Bounded.from(e)` proves `e % c`. This is the
   smallest step that pays for itself; it has a measured target in fasta.
   Grow `from`'s rule list one measured rule at a time (`e & mask`, a widened
   `u8`), with one rule set shared by the checker and the code generator so
   that what compiles and what is omitted cannot disagree (the issue #863
   lesson).
2. **Optionally, closed constant expressions in value arguments.**
   `Bounded[i64, 0, IM - 1]`. Cheap and independent; take it if the
   `IM_LAST` constants become common.
3. **Then measure before building `Index[v]`.** Step 1 of the measurement plan
   costs no compiler change. Proceed only if a whole program gains CPU time.
4. **If it pays, ship `Index[v]` as specified above:** view bindings only,
   closed introduction list, bare dependent arguments, erased before code
   generation.
5. **Defer length relations** (`len(dst) >= len(src)` in signatures, two views
   sharing one index type) until a kernel shows the versioned copy costs CPU
   time, not only code size.
6. **Do not adopt** refinement types with a solver, or full dependent types.

### Non-goals

- No unchecked constructor, `assume`, or trusted assertion for any of these
  types.
- No flow narrowing from `if` guards.
- No proofs of data invariants: search termination, sortedness, permutation
  contents, triangular counters.
- No dependency on owned or resizable containers; `std.graph` identifiers
  stay with [checked node identities](completed/arenas.md).
- No change to the type of existing loops or `std` returns without a separate
  decision.

## Open questions

1. Is a sparse or gather-heavy whole program a goal Plenty should optimize
   for? Without one, `Index[v]` has a kernel gain but no program to justify it.
2. Should `std` gain typed variants (`v.position(x) -> Option[Index[v]]`,
   `rng.index_of(v) -> Option[Index[v]]`), or should existing functions change
   their return types in an edition?
3. Accept closed constant expressions in value arguments now, or keep
   requiring a named constant?
4. Should `array[T, N]` get a built-in index type when it ships, which would
   need either derived sizes or a special case?
5. Is the name `Index` right? It is not taken in `std` (searched the
   signature index), but it is a common user class name.
6. After issue #945 step 2, should view classes allow field-path brands such
   as `Index[self.data]`?

## Rejected alternatives

| Alternative | Reason |
| --- | --- |
| Bounds from any runtime value (`Bounded[i64, 0, n]` with mutable `n`) | The type would change meaning when `n` changes; needs flow-sensitive types |
| `Bounded[i64, 0, n]` with immutable `n` as the main design | Removes no overflow checks and no bounds checks without length relations |
| Naming owned lists or `&mut list[T]` | They can resize; would need a new length-only loan |
| Flow narrowing from guards | Reverses the issue #796 decision; no cheaper than one `try_from` |
| SMT-backed refinements | Compile time, dependency size, unpredictable acceptance |
| Full dependent types | Size of the checker and learner burden |
| Programmer-asserted facts (`assume i < len(v)`) | An unchecked escape hatch; `unchecked:` already covers deliberate assumptions |
