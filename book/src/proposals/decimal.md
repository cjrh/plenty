# Exact decimal arithmetic (`std.decimal`)

> **Status: Not implemented.** Nothing shipped; no reference page exists.
> Researched against local `main` at `70e85758` on 2026-10-31. Dataframe
> decimal columns wait on this design ([B76](../backlog.md)). No backlog
> entry exists yet. Revised twice on 2026-10-31 after review; every
> [decision](#decisions) is now settled.

## Recommendation

Add `std.decimal` with one type, `Decimal`: a fixed-size, exact decimal of up
to 38 significant digits and 0–38 fractional digits, written in Plenty as an
allocation-free [copy class](../design/language/03-types/classes.md#copy-classes).
It follows CPython's `decimal` wherever that does not conflict with the rules
below, since CPython's semantics are the most widely used and tested:

- The scale is kept, as in Python: `Decimal.parse("1.50")` displays as
  `1.50`, and `1.5 * 2.00` is `3.000`. Equality and ordering are numeric
  across scales (`1.0 == 1.00`), and equal values hash equal, as in Python.
- `+`, `-` and `*` are exact. A result that does not fit traps as integer
  overflow does, and inside a [`checked:` block](../design/language/03-types/numbers.md#checked-blocks)
  it becomes `Err(OverflowError[Decimal])`, as for `i64`. There are no
  decimal-specific checked methods, contexts or signals.
- Division takes a [`NonZero[Decimal]`](../design/language/03-types/numbers.md#nonzero-integers)
  divisor, so division by zero cannot be written.
- Nothing rounds unless the call names places and a rounding mode, including
  conversion from `f64`. Division therefore has no `/` operator; it is
  `a.div(divisor, places, mode)`. There is no context, no precision setting,
  and no silent rounding.
- There is no NaN, infinity, or negative zero.
- The digit limit matches Arrow/Polars `Decimal128` and SQL `DECIMAL(38, s)`,
  so dataframe and database values convert without loss.

Do not wrap `rust_decimal`, libmpdec, or decNumber for the default type.
Leave arbitrary precision to a separate, later design if a workload needs it.

The type needs language work first (staged plan below): a 64×64→128-bit
integer multiply; `@arithmetic` methods and hooks, so library types join the
integer trap and `checked:` rules; the `__cmp__` hook from the
[ordering proposal](user-type-ordering.md), so `==` is numeric; and a wider
`NonZero` bound. Dictionary keys and decimal format specs follow later.

## What financial code needs

| Need | Why | This design |
|---|---|---|
| Exact representation of decimal fractions | `0.1 + 0.2` must equal `0.3`; amounts must round-trip through text | Coefficient × 10<sup>-scale</sup>, scale kept |
| Exact addition, subtraction and multiplication | Totals, quantities × prices, rates × amounts | Exact or overflow; never rounded |
| Explicit rounding with named modes | Tax, interest and currency rules specify both the place and the rule (banker's vs commercial) | `quantize(places, mode)`; seven modes |
| Division to stated places | Unit prices, rates, averages | `div(divisor, places, mode)` with a `NonZero[Decimal]` divisor |
| Overflow detection | A wrong total is worse than a crash | The integer trap, or `Err` inside `checked:`; never wrap |
| No surprise from binary floats | `0.1` as `f64` is not 0.1 | `from_f64` requires places and a mode |
| Parsing and formatting without loss | CSV, JSON, SQL, user input | `parse` keeps every digit or fails; display keeps the scale |
| Splitting an amount without losing units | Instalments, pro-rata allocation | `allocate` (stage 2) |
| Interchange with columnar and SQL decimals | Polars, Parquet, databases | Same 38-digit range; scale kept like a column's |

Not needed for the common case, and excluded from the default: transcendental
functions (`ln`, `exp`, `sqrt`), NaN payloads, signalling NaNs, subnormals,
precision contexts, and numbers beyond 38 digits.

## Candidates

The user's phrase "the Rust `decimal` crate" most likely means
[`rust_decimal`](https://crates.io/crates/rust_decimal) (168 M downloads, the
de facto Rust choice). The crate actually named
[`decimal`](https://crates.io/crates/decimal) is an older binding to IBM's
decNumber `decimal128` (last release 2.1.0, 2021). Both are covered.

| Candidate | Model | Size and allocation | Rounding and errors | Licence, form |
|---|---|---|---|---|
| CPython `decimal` / [libmpdec](https://www.bytereef.org/mpdecimal/) | General Decimal Arithmetic (GDA) spec, arbitrary precision; context with precision (default 28), rounding (default half-even), traps and sticky flags; NaN, ±Inf, −0; trailing zeros significant | Heap coefficient per value. Allocation is fallible: `mpd_switch_to_dyn` sets `MPD_Malloc_error` and returns NaN (`mpalloc.c`) | Every operation rounds to context precision; traps raise, otherwise flags are set. Python keeps the context in a `contextvars` variable: hidden per-thread state | BSD-2-Clause C, about 18,600 lines. CPython removed its bundled copy in May 2026 (`9eb3b146686`) and now requires a system libmpdec ≥ 2.5 |
| decNumber via [`decimal`](https://crates.io/crates/decimal) or [`dec`](https://crates.io/crates/dec) | GDA; fixed `decimal128` (34 digits, IEEE 754-2008) plus arbitrary `decNumber` | `decimal128` is 16 bytes and allocation-free; `decNumber` may `malloc` | Context and status flags, as libmpdec | ICU licence C (about 16,800 lines), Apache-2.0 bindings; `decimal` unmaintained since 2021 |
| [`rust_decimal`](https://crates.io/crates/rust_decimal) 1.43 | 96-bit coefficient, scale 0–28, 16 bytes; at most 28–29 digits | No allocation for arithmetic | **Rounds silently**: a product whose scale exceeds 28 is rounded half-even, and one too small becomes zero (`src/ops/mul.rs`); division rounds to 28 digits. Operators panic on overflow; `checked_*` return `Option`. Eq/Hash are numeric (normalized) | MIT Rust, about 12,300 lines |
| [`fastnum`](https://crates.io/crates/fastnum) 0.7 | `D128`: 128-bit coefficient plus a 64-bit control block (scale, sign, flags), 24 bytes; NaN, ±Inf, ±0 | No allocation | GDA-like context and signals. Default traps (division by zero, invalid, overflow) panic **only in debug builds** (`context/signal_traps.rs`); release builds return special values | MIT/Apache-2.0 Rust, about 25,700 lines; MSRV 1.94 |
| [`bigdecimal`](https://crates.io/crates/bigdecimal) | `num-bigint` coefficient + `i64` scale | `Vec` per value; Rust's global allocator aborts on failure | Explicit precision on division | MIT/Apache-2.0 Rust |
| Polars `Decimal(p, s)` (`polars-compute` 0.55.2 `decimal.rs`) | `i128` unscaled values, precision ≤ 38, **scale fixed per column** | Column storage | Multiplication and division round half-even back to the column scale; overflow returns `None` | MIT Rust; reference algorithms, not an API |
| **Plenty-native `Decimal`** (recommended) | Two base-10<sup>19</sup> `u64` limbs + scale + sign, as libmpdec stores words; 38 digits, scale 0–38, scale kept | Four inline fields; never allocates | Exact or trap; rounding only on request | Plenty source in `stdlib/std/decimal.plenty` |

### Why not wrap

Project policy (`AGENTS.md`, [engine criteria](../design/toolchain/01-building/engines.md))
requires a concrete reason to wrap, such as substantial complexity or
specialist security work. A fixed 128-bit decimal has neither:

- The core is integer arithmetic on two 64-bit limbs plus scaling by powers of
  ten. Polars implements the equivalent `i128` operations, including
  correctly rounded 256-by-128-bit division, in one 1,171-line file.
- Every wrapped candidate carries semantics the recommended rules reject:
  `rust_decimal` rounds silently and stops at 28 digits, below the 38 that
  Polars and SQL use; `fastnum` returns NaN in release builds; libmpdec and
  decNumber need a context on every call and C builds for four targets,
  including MSVC.
- A native call per addition costs more than the addition. A Plenty
  implementation inlines through ordinary Cranelift code.
- Plenty code links per module, so a program that does not import
  `std.decimal` pays nothing either way (per-module linking, checked by
  `tests/test_executable_size.rs`).

Wrapping libmpdec remains the right choice if users later need the full GDA
operation set (`ln`, `exp`, `sqrt`, `pow` with fractional exponents) at
arbitrary precision. Its allocation failure is already reported as a status
flag, so a wrapper can map it to `AllocError` without patching.

## Semantic choices

### Fixed 38 digits, not arbitrary precision

Fixed size is what makes the type a copy class: no allocation, no `Result`
on every operation, no ownership bookkeeping. 38 digits holds any amount below
10<sup>20</sup> currency units with 18 fractional digits (enough for
Ethereum's 10<sup>-18</sup>), and is the limit of Arrow `Decimal128`, Polars,
Parquet, SQL Server, BigQuery `NUMERIC` and Snowflake `NUMBER`. IEEE
`decimal128` has only 34 digits and a binary-incompatible encoding (BID/DPD);
`rust_decimal` has 28.

Arbitrary precision moves allocation into every arithmetic expression,
`(a * b)?` everywhere, and needs a rule for how large results may grow. If a
workload needs it, design it separately; this proposal does not depend on
one.

### Representation

Store the coefficient in two `u64` words in base 10<sup>19</sup>, as libmpdec
does on 64-bit targets (`MPD_RADIX` is 10<sup>19</sup>, `MPD_RDIGITS` 19, in
`mpdecimal.h`). Two words hold exactly 38 digits, so the digit limit is "high
word below 10<sup>19</sup>"; aligning scales, rounding at a digit, and parsing
and printing split at word boundaries with powers of ten that fit one word.
libmpdec multiplies words with a double-word product (`_mpd_mul_words` in
`typearith.h`: `__uint128_t` or x86-64 `mulq`); `u64.widening_mul` is the
Plenty equivalent. Reducing a product by the constant 10<sup>19</sup> is a
multiply by a precomputed reciprocal, so the same primitive serves; the
prototype should confirm that before choosing between base 10<sup>19</sup> and
binary words. This is an implementation choice, not part of the API.

### Scale, equality and hashing

The scale is kept, with Python's ideal exponents (`Lib/_pydecimal.py`):

| Operation | Result scale | Example |
|---|---|---|
| `parse` | As written | `"1.50"` → `1.50` |
| `a + b`, `a - b` | `max(scale_a, scale_b)` | `1.5 + 2.25` → `3.75`; `1.50 + 1.50` → `3.00` |
| `a * b` | `scale_a + scale_b` | `1.5 * 2.00` → `3.000` |
| `quantize(places, mode)`, `div(d, places, mode)` | `places` | `1.5.quantize(2, …)` → `1.50` |
| `normalize()` | Smallest exact scale | `3.000` → `3` |

A result needing more than 38 digits, counting trailing zeros, or more than 38
fractional digits overflows. Python would round it to the context precision
instead; here that is a trap, or `Err` in `checked:`.

Python compares numerically: `Decimal("1.0") == Decimal("1.00")` is `True`
and `hash(Decimal("1.0")) == hash(Decimal("1.00"))` (`__eq__` uses `_cmp`;
`__hash__` hashes coefficient × 10<sup>-exp</sup> modulo the hash prime).
`Decimal` matches both. This needs three things from Plenty:

1. **Equality and ordering: `__cmp__`.** Plenty's `==` on classes is
   structural and cannot be overridden
   ([operator boundaries](../design/language/04-expressions/operators.md#boundaries)),
   so with the scale kept, structural `==` would make `1.0 == 1.00` false.
   The [ordering proposal](user-type-ordering.md) (B37) already gives the fix:
   defining `__cmp__(self, other: &T) -> Order` derives all six comparisons,
   and replaces structural `==` and `!=` for the type (its rule 3), including
   inside collections. `Decimal.__cmp__` aligns scales and compares
   coefficients; it allocates nothing. `Decimal` cannot ship before this,
   because its `==` would be wrong. A separate `__eq__` is not needed and the
   ordering proposal rejects one; Python's own six methods all reduce to
   `_cmp`.
2. **Keys: a key projection.** Under the [hashing proposal](user-type-hashing.md)
   (B74), keys hash structurally, so a type defining `__cmp__` is not a key
   type. Its [key projection](user-type-hashing.md#possible-later-extension-key-projection)
   extension is the consistent route: `Decimal.__key__()` returns the
   normalized parts (coefficient without trailing zeros, scale, sign), a
   structural key value, so equal decimals hash and look up equally. That
   proposal's main objection, that projections usually allocate, does not
   apply: normalizing is integer division by ten on two words. Until it lands,
   `Decimal` is not a key; code can key on `d.normalize().parts()`.
3. **Consistency.** `__key__` equality and `__cmp__` equality must agree,
   which the compiler cannot check. Restrict both hooks to `std` at first, as
   for `@arithmetic`, and test the agreement with differential tests against
   CPython.

### No context, no special values

A GDA context is hidden mutable state (Python stores it in a `contextvars`
variable), and its default precision of 28 silently rounds results that do
not fit. Instead, an operation that can be inexact takes its places and
rounding mode as arguments, and the others are exact. With no context there
are no sticky flags to check, and with errors as values there is no need for
NaN or infinity. Negative zero carries no financial meaning: `parse("-0.00")`
is `0.00`, where Python keeps `-0.00` (it still compares equal to zero).

### Floats

Python's `Decimal(0.1)` and `Decimal.from_float(0.1)` convert exactly, giving
`0.1000000000000000055511151231257827021181583404541015625`; mixing floats
into the constructor and comparisons is allowed unless the `FloatOperation`
signal is trapped. That exact expansion is the confusion this module exists
to avoid, and at 55 digits it does not fit anyway.

`Decimal` has no implicit or exact float conversion:

- `Decimal.from_f64(value, places, mode)` rounds the float's exact binary value
  to `places` with `mode`: `from_f64(0.1, 2, Rounding.HalfEven)` is `0.10`.
  NaN, infinity, or a result over 38 digits is `Err(RangeError.OutOfRange)`.
- Operators and comparisons never accept `f64`; mixing is a compile error, as
  mixing `i64` and `f64` is.
- `parse` is the way to write a decimal constant: `Decimal.parse("0.1")`.
- `to_f64()` returns the nearest `f64`.

### Overflow: the integer trap and `checked:`

`+`, `-`, `*` and unary `-` on `Decimal` follow the
[integer rules](../design/language/03-types/numbers.md#checked-blocks):

- Outside a `checked:` block, an unrepresentable result is an arithmetic trap
  at the caller's position, with the
  [standard-library caller note](../design/language/08-errors/02-allocation-and-traps.md#trap-reports).
  Actors and test harnesses see it as `TrapKind.Arithmetic`, like integer
  overflow.
- Written inside a `checked:` block, the same expression makes the block
  `Err(OverflowError[Decimal])`. A block mixing integer and decimal
  arithmetic gets both errors in its anonymous sum, for example
  `OverflowError[i64] | OverflowError[Decimal]`.

The same rule covers the methods that can overflow (`quantize`, `div`,
`mul_quantize`, `div_rem`), as it covers `pow` on integers.

`OverflowError[T]`'s `wrapped` field is the exact result modulo
2<sup>N</sup> for integers. For `Decimal` it is the decimal analogue: the
exact result truncated toward zero to 38 fractional digits, with its
coefficient reduced modulo 10<sup>38</sup>. It is deterministic and, like the
integer field, mainly diagnostic.

#### Gap: library types in the trap rules

Today the rules do not reach library code. A `checked:` block checks only
builtin integer operators, `pow`, and operators that `@newtype` classes
forward; an operator hook such as `Complex.__add__` is an ordinary call and
keeps trapping inside a block. Standard-library code also has no way to raise
the arithmetic trap for a condition that is not an integer overflow, such as a
39-digit decimal result.

The proposed fix is one general mechanism, not a decimal one. An
`@arithmetic` decorator on a method or operator hook whose body returns
`Result[T, OverflowError[T]]` (optionally `| ZeroDivisionError`) changes how
callers see it:

- Callers see the method as returning `T`. Outside `checked:`, the compiler
  turns an `Err` into the arithmetic trap at the call site, so the caller note
  and `TrapKind.Arithmetic` work as for integers.
- A call or operator written lexically in a `checked:` block adds the declared
  errors to the block's error type and exits the block on `Err`, as integer
  operators do.
- The lexical rule is unchanged: integer arithmetic inside the hook body is
  ordinary code, and the hook decides what overflow means for its type.
- Users never call a separate checked form. `checked:` is the only way to
  receive the error.

Closing the gap needs checker work (validate the decorator's signature, type
the call as `T`, contribute errors to enclosing blocks, and restrict the
decorator to `std`) and code generation (the `Err` branch to the existing
trap and the block's exit edge). Other library types could adopt
it later; `std.time.Duration`, which has `checked_*` methods returning
`RangeError`, is one candidate, out of scope here.

### Rounding modes

Seven modes, CPython's set without `ROUND_05UP`, named by direction rather
than by Python's ambiguous `ROUND_UP` and `ROUND_DOWN`:

| Plenty | Python / GDA | Example at 0 places: 2.5, −2.5, 2.4 |
|---|---|---|
| `HalfEven` | `ROUND_HALF_EVEN` | 2, −2, 2 |
| `HalfAwayFromZero` | `ROUND_HALF_UP` | 3, −3, 2 |
| `HalfTowardZero` | `ROUND_HALF_DOWN` | 2, −2, 2 |
| `TowardZero` | `ROUND_DOWN` | 2, −2, 2 |
| `AwayFromZero` | `ROUND_UP` | 3, −3, 3 |
| `Floor` | `ROUND_FLOOR` | 2, −3, 2 |
| `Ceiling` | `ROUND_CEILING` | 3, −2, 3 |

GDA's `ROUND_05UP` is omitted. Every rounding call names its mode; there is no
default.

### Division: a non-zero divisor

Division by zero is ruled out by the divisor's type, following the decision
behind `NonZero[T]` (issue #584): check once where the value is created, not
at every use. The divisor is `NonZero[Decimal]`, the same builtin type the
integers use, because widening `NonZero` to library types later lets users
define non-zero forms of their own numeric types.

#### Widening the `NonZero` bound

`NonZero[T: IntType]` becomes `NonZero[T: ZeroTest]`. `ZeroTest` is a built-in
constraint, like `IntType` and the proposed `KeyType`, because a protocol cannot
admit integers (they have no methods). It is satisfied by:

- every integer type, unchanged: literals convert at compile time, `0` is a
  compile error, and operators forward to the builtin integer operators;
- a `@copy` class that defines `def is_zero(self) -> bool`, computation only
  (no I/O, clock, entropy or process effects, as `__cmp__` requires).

For such a class `T`, `NonZero[T]` provides what it provides for integers,
built from `T`'s methods:

| `NonZero[i64]` | `NonZero[Decimal]` |
|---|---|
| Literal converts at compile time; `0` is a compile error | No decimal literals. `Decimal.nonzero(units: NonZero[i64], scale)` and `Decimal.nonzero_int(value: NonZero[i64])` take integer units, so `Decimal.nonzero(0, 2)` is a compile error |
| `NonZero.try_from(value) -> Result[NonZero[i64], RangeError]` | Same; `OutOfRange` when `is_zero()`, so `0.00` is rejected |
| `.get() -> i64` | `.get() -> Decimal` |
| `*` and unary `-` keep the type; `+` and `-` return `T` | Same, through `Decimal`'s hooks |
| Mixed `NonZero[T]` and `T` operands are a compile error suggesting `.get()` | Same |
| Copy, `==`, ordering, printing, dictionary key | Copy; `==` and ordering from `Decimal.__cmp__`; printing; keys when `Decimal` is one |

`Decimal` must provide `@copy`, `is_zero` (the coefficient is zero, at any
scale), and `*` and unary `-` hooks that never turn non-zero operands into
zero. Both hold: products are exact or overflow, and a product needing more
than 38 fractional digits overflows instead of rounding to zero. Inside
`checked:`, `NonZero[Decimal]` products report `OverflowError[Decimal]`, as
`NonZero[i64]` products report `OverflowError[i64]`.

That last requirement is a contract the compiler cannot check. A user type
whose `*` can produce zero (a matrix, a modular integer) would make
`NonZero[T]` lie. So `ZeroTest` for classes is `std`-only at first, like
`@arithmetic`; opening it to user types needs a rule for that contract.

The compiler work is in `src/frontend/nonzero.rs`: the bound, `try_from`
through `is_zero`, forwarding `*`, unary `-`, `+`, `-` and comparisons through
the class's hooks rather than the integer operators, and the compile-time
`0` check staying integer-only.

There is no flow narrowing (`if d != 0:` does not change `d`'s type), as for
integers. A runtime count converts once: `Decimal.nonzero_int(NonZero.try_from(n)?)`.

#### Division API

```text
a.div(divisor: &NonZero[Decimal], places: i64, mode: Rounding) -> Decimal
a.div_rem(divisor: &NonZero[Decimal], mode: Rounding) -> (Decimal, Decimal)
```

- `div` returns the quotient correctly rounded to `places` fractional digits
  (0–38; others trap like `Decimal.of`) with `mode`. A quotient over 38 digits
  overflows under the `checked:` rules above; it has no other failure.
- `div_rem` rounds the quotient to an integer with `mode` and returns the
  exact remainder `a - q * divisor`. `TowardZero` gives Python's `Decimal`
  `//` and `%` (`-7 // 2` is `-3`, remainder `-1`); `Floor` gives Plenty's
  integer `//` and `%` (`-4`, remainder `1`). The two disagree, so the mode is
  required rather than picking one.

Why still no `/`: a non-zero divisor removes the zero case but not rounding.
`1 / 3` has no exact result, so `/` would need fixed places and a fixed mode.
Python rounds to the context precision, Polars to the column scale and
`rust_decimal` to 28 digits; each is the silent rounding this design excludes,
and a trap on every inexact quotient would make `/` fail for most inputs.

Integers differ today: `i64 // NonZero[i64]` is a compile error (the operand
must be `i64`), and integer `//` and `%` still trap on zero. Issue #584 left
`NonZero` divisors for `//` and `%` as separate work; `std.decimal` starts with
the rule that work would adopt.

### Parsing and formatting

`Decimal.parse(text)` follows the [numeric parsing contract](../design/facilities/01-builtins/numbers.md#parsing):
it allocates nothing, trims white space, and accepts an optional sign, digits,
an optional fraction, and an optional exponent (`1.5e3`). The scale is the
written one, adjusted by the exponent and never below 0: `"1.50"` has scale 2,
`"1.5e3"` is `1500`, `"1E+2"` is `100` (Python keeps exponent 2 and prints
`1E+2`). More than 38 digits or 38 fractional digits is `ParseError.OutOfRange`;
it never rounds. `inf`, `nan`, grouping and locale forms are
`ParseError.Invalid`.

`display` writes the plain form with the stored scale (`-1234.50`), never an
exponent; Python switches to exponent form for small values (`1E-7`). To show
a fixed number of places, quantize first: `total.quantize(2, Rounding.HalfEven)`.

Today an f-string spec on a class applies to its text: probing with
`std.complex`, `f"{Complex(1.25, 2.0):.3}"` prints `1.2`. For a decimal,
`{total:.2}` would therefore cut `64.92` to `64`. Until format specs reach
library types (stage 3), the reference must warn against specs on `Decimal`.

### Naming against Python

| Python | Plenty | Reason |
|---|---|---|
| `quantize(Decimal("0.01"), rounding)` | `quantize(2, mode)` | Same operation and name; places as a count, since scale is never negative |
| `round(d, n)` | Not provided | Python's uses the context's rounding, and `round(d)` returns `int` |
| `normalize()` | `normalize()` | Same, but exact: no rounding to a precision |
| `compare`, `<`, `==` | `compare`, `<`, `==` from `__cmp__` | Same |
| `int(d)` | `to_i64(mode)` | `int` truncates silently |
| `as_tuple()` | `parts()` | Coefficient words, scale, sign |
| `ROUND_HALF_UP` etc. | `Rounding.HalfAwayFromZero` etc. | Python's `UP`/`DOWN` names read as directions they are not |

## Fit with the language today

| Need | Today | Consequence |
|---|---|---|
| 128-bit coefficient | No `u128`/`i128`; `u64` has wrapping and checked operations but no high product | Two `u64` words; add `u64.widening_mul` (Cranelift `umulhi`, available on x86-64 and AArch64 in the pinned 0.137) |
| Arithmetic operators | [Operator hooks](../design/language/04-expressions/operators.md) as in `std.complex` | `+ - *` and unary `-` |
| `checked:` and traps | Integer operators, `pow`, `@newtype` forwarding; hooks keep trapping; std cannot raise the arithmetic trap | `@arithmetic` methods and hooks, `std` only |
| Numeric `==` and `<` | Structural `==` only; no ordering hooks ([B37](../backlog.md)) | `__cmp__` from the [ordering proposal](user-type-ordering.md); a prerequisite |
| Dictionary keys | Not available for classes ([B74](../backlog.md)) | `__key__` projection after B74 |
| Non-zero divisor | `NonZero[T]` for integer `T` only | `ZeroTest` bound, `std` classes only |
| Literals and constants | `const` accepts only scalars, `str`, `bytes` | `Decimal.of(1999, 2)` is allocation-free and infallible; `parse` for text |
| Format specs | Applied to display text | `quantize` then display; later a spec hook |

Decimal needs `widening_mul` on its own account: every product of two
coefficients, and the 256-by-128-bit division behind `div` and `quantize`, is
built from 64×64→128-bit products.

## API sketch

```text
from std.decimal import Decimal, Rounding

pub enum Rounding:
    HalfEven
    HalfAwayFromZero
    HalfTowardZero
    TowardZero
    AwayFromZero
    Floor
    Ceiling

@copy
pub class Decimal:
    # private: _high: u64, _low: u64 (base 10**19), _scale: u8, _negative: bool

    # Construction: allocation-free.
    Decimal.ZERO, Decimal.ONE
    Decimal.of(units: i64, scale: i64) -> Decimal        # of(1999, 2) is 19.99; traps outside 0..38
    Decimal.from_int(value: i64) -> Decimal
    Decimal.nonzero(units: NonZero[i64], scale: i64) -> NonZero[Decimal]
    Decimal.nonzero_int(value: NonZero[i64]) -> NonZero[Decimal]
    Decimal.parse(text: str) -> Result[Decimal, ParseError]
    Decimal.from_f64(value: f64, places: i64, mode: Rounding) -> Result[Decimal, RangeError]
    Decimal.from_parts(high: u64, low: u64, scale: i64, negative: bool) -> Result[Decimal, RangeError]

    # Exact arithmetic. @arithmetic: traps on overflow; Err(OverflowError[Decimal]) inside checked:.
    a + b, a - b, a * b, -a
    a.abs() -> Decimal

    # Rounding is always explicit. Also @arithmetic.
    a.quantize(places: i64, mode: Rounding) -> Decimal
    a.div(divisor: &NonZero[Decimal], places: i64, mode: Rounding) -> Decimal
    a.mul_quantize(&b, places: i64, mode: Rounding) -> Decimal   # exact product, then quantize
    a.div_rem(divisor: &NonZero[Decimal], mode: Rounding) -> (Decimal, Decimal)

    # Comparison: __cmp__ gives == != < <= > >= numerically across scales.
    a.compare(&b) -> Order
    a.is_zero() -> bool               # ZeroTest
    a.normalize() -> Decimal          # strip trailing fractional zeros; exact

    # Inspection and conversion.
    a.sign() -> i64
    a.scale() -> i64                  # stored scale
    a.is_integer() -> bool
    a.parts() -> (u64, u64, i64, bool)
    a.to_i64(mode: Rounding) -> Result[i64, RangeError]
    a.to_f64() -> f64                 # nearest f64
    a.to_units(scale: i64) -> Result[(u64, u64, bool), RangeError]   # unscaled value at a fixed scale; fails if not exact

    # Text.
    a.display(out)                    # plain form with the stored scale
```

Conversions report `RangeError.OutOfRange`, as integer `try_from` does; there
is no `DecimalError`. No arithmetic reports division by zero, since no
operation accepts a zero divisor.

Example, in the proposed API:

```text
from std.decimal import Decimal, Rounding
from std.fmt import FormatError

def unit_price(total: Decimal, count: i64) -> Result[Decimal, RangeError]:
    per = Decimal.nonzero_int(NonZero.try_from(count)?)
    Ok(total.div(&per, 2, Rounding.HalfEven))

def main() -> Result[(), IoError | ParseError | RangeError | FormatError]:
    price = Decimal.parse("19.99")?
    rate = Decimal.parse("0.0825")?
    subtotal = price * Decimal.from_int(3)                 # 59.97
    tax = (subtotal * rate).quantize(2, Rounding.HalfEven) # 4.947525 -> 4.95
    total = subtotal + tax
    print(f"{total}"?)?                                    # 64.92
    print(f"{unit_price(total, 7)?}"?)?                    # 9.27
    half = Decimal.parse("1.50")?
    print(f"{half}"?)?                                     # 1.50
    print(half == Decimal.of(15, 1))?                      # True
    Ok(())

# Errors as values instead of a trap, with the integer rule.
def grand_total(lines: &list[Decimal]) -> Result[Decimal, OverflowError[Decimal]]:
    checked:
        total = Decimal.ZERO
        for line in lines:
            total += line
        total
```

### Interchange

- **Polars/Parquet** ([B76](../backlog.md)): enable Polars' `dtype-decimal`
  feature in `plenty-dataframe` and measure the dataframe executable's growth.
  A column `Decimal(p, s)` reads as `Decimal` values with scale `s`, since
  the scale is kept; writing to a column needs each value at scale `s`
  exactly (`to_units(s)` fails with `RangeError` otherwise), so the caller
  quantizes first. Column arithmetic stays in Polars, with its own
  half-even rounding to the column scale, and the reference must say so.
- **JSON**: `Event.Number` already exposes number text
  ([std.json](../design/stdlib/json.md)); add a decimal reader and writer
  without going through `f64`.
- **CSV, SQLite**: parse fields and bind as text. SQLite has no exact decimal
  storage class; binding through `REAL` would lose digits.

## Licences, size and platforms

A Plenty implementation ships no third-party code. If it ports algorithms from
Polars' `decimal.rs` (MIT), credit Polars in the module header, the reference
page's acknowledgements and `scripts/release/third_party_notices.py`. Tests
can use the GDA `decTest` vectors and CPython's `decimal` as a differential
oracle (configured to 38 digits with traps for `Inexact`), without shipping
either. Code size reaches only programs that import the module. The
implementation uses only integer operations, so it behaves identically on
Linux x86-64 and ARM64, macOS ARM64 and Windows MSVC; C candidates would need
per-target builds and MSVC configuration.


## Staged plan

1. **Prerequisites.**
   - `u64.widening_mul(other) -> (u64, u64)` (high, low), lowered to
     `umulhi` and `imul`. Check the generated code before timing.
   - `@arithmetic` methods and operator hooks, `std` only
     ([gap](#gap-library-types-in-the-trap-rules)), with tests on a small
     test-only class: trap position and caller note, `TrapKind.Arithmetic`,
     `checked:` error types, and the lexical rule.
   - `__cmp__` from the [ordering proposal](user-type-ordering.md), at least
     for `std` classes, so `Decimal`'s `==` is numeric.
   - The `ZeroTest` bound for `NonZero`, `std` classes only.
2. **Core module.** `Decimal` and `Rounding`; parsing and display with the
   stored scale; exact `+ - *`; `quantize`, `div`, `mul_quantize`, `div_rem`,
   `normalize`, conversions; `allocate(parts)` for splitting an amount into
   parts that sum exactly. Reference page, a tutorial lesson replacing `f64`
   money arithmetic, differential tests against CPython (38 digits, traps for
   `Inexact` and `Rounded`), allocation tests proving no allocation.
3. **Language integration.** Dictionary keys through `__key__` once B74 covers
   classes; a format-spec hook so `{total:.2f}` quantizes (half-even, as
   Python's `format` does) instead of truncating text.
4. **Interchange.** Polars decimal columns (B76), JSON number text, CSV
   helpers.
5. **Only on evidence.** A standalone arbitrary-precision decimal design, or a
   fallible libmpdec wrapper for the full GDA operation set; a decimal literal
   suffix (`19.99d`) if `parse` and `of` prove too noisy; `@arithmetic` and
   `ZeroTest` for user types.

## Decisions

Decided in review (2026-10-31):

1. **Scale.** Kept, as in Python. Equality and ordering are numeric across
   scales through `__cmp__`, and keys hash the normalized value through
   `__key__`.
2. **Digit limit.** 38 digits.
3. **Rounding mode.** Always explicit, including `from_f64` and `div_rem`.
4. **Multiply primitive.** `u64.widening_mul`, as libmpdec uses a double-word
   multiply on base-10<sup>19</sup> words.
5. **Non-zero divisor.** `NonZero[Decimal]`, through a `ZeroTest` bound;
   `std` classes now, user types later.
6. **`wrapped`.** Truncated to 38 fractional digits, coefficient modulo
   10<sup>38</sup>.
7. **`@arithmetic`.** `std` only for now.
8. **Overflow** traps like integers and is `Err(OverflowError[Decimal])`
   inside `checked:`; no `checked_*` methods, contexts or signals.
9. **Division** is `div(divisor, places, mode)`; no `/`.
10. **No coupling to other numeric proposals.** Arbitrary precision is a
    separate later design.

Still open, none blocking stage 1:

1. **Rounding names.** Direction names (recommended) or CPython's
   `ROUND_HALF_UP` style.
2. **User types.** The rule for opening `@arithmetic`, `ZeroTest` and
   `__key__` to user code: each rests on a contract the compiler cannot check
   (non-zero-preserving `*`, `__key__` agreeing with `__cmp__`).
3. **Word base.** Base 10<sup>19</sup> (recommended, as libmpdec) or binary
   words, settled by the prototype; not user-visible.
