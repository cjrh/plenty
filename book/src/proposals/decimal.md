# Exact decimal arithmetic (`std.decimal`)

> **Status: Not implemented.** Nothing shipped; no reference page exists.
> Researched against local `main` at `70e85758` on 2026-10-31. Dataframe
> decimal columns wait on this design ([B76](../backlog.md)). No backlog
> entry exists yet. Revised on 2026-10-31 after review: a non-zero divisor
> type, the existing trap and `checked:` rules, and a standalone design.

## Recommendation

Add `std.decimal` with one type, `Decimal`: a fixed-size, exact decimal of up
to 38 significant digits and 0–38 fractional digits, written in Plenty as an
allocation-free [copy class](../design/language/03-types/classes.md#copy-classes).
Its rules are deliberately few:

- `+`, `-` and `*` are exact. A result that does not fit traps as integer
  overflow does, and inside a [`checked:` block](../design/language/03-types/numbers.md#checked-blocks)
  it becomes `Err(OverflowError[Decimal])`, as for `i64`. There are no
  decimal-specific checked methods, contexts or signals.
- Division takes a `NonZeroDecimal` divisor, the decimal counterpart of
  [`NonZero[T]`](../design/language/03-types/numbers.md#nonzero-integers), so
  division by zero cannot be written.
- Nothing rounds unless the call names places and a rounding mode. Division
  therefore has no `/` operator; it is `a.div(divisor, places, mode)`. There
  is no context, no precision setting, and no silent rounding.
- There is no NaN, infinity, or negative zero. Invalid operations are errors.
- Values are stored canonically, without trailing fractional zeros, so the
  structural `==` that Plenty gives every class is numeric equality, and
  `Decimal` can become a dictionary key under the planned structural key
  rules ([B74](../backlog.md)) with no hashing hook.
- The digit limit matches Arrow/Polars `Decimal128` and SQL `DECIMAL(38, s)`,
  so dataframe and database values convert without loss.

Do not wrap `rust_decimal`, libmpdec, or decNumber for the default type.
Leave arbitrary precision to a separate, later design if a workload needs it.

The type needs two language additions first: a 64×64→128-bit integer
multiply, and a way for a library method or operator hook to take part in
the integer trap and `checked:` rules. It gains comparison operators and
decimal format specs from separate language work (staged plan below).

## What financial code needs

| Need | Why | This design |
|---|---|---|
| Exact representation of decimal fractions | `0.1 + 0.2` must equal `0.3`; amounts must round-trip through text | Coefficient × 10<sup>-scale</sup> |
| Exact addition, subtraction and multiplication | Totals, quantities × prices, rates × amounts | Exact or overflow; never rounded |
| Explicit rounding with named modes | Tax, interest and currency rules specify both the place and the rule (banker's vs commercial) | `round(places, mode)`; seven modes |
| Division to stated places | Unit prices, rates, averages | `div(divisor, places, mode)` with a `NonZeroDecimal` divisor |
| Overflow detection | A wrong total is worse than a crash | The integer trap, or `Err` inside `checked:`; never wrap |
| Parsing and formatting without loss | CSV, JSON, SQL, user input | `parse` rejects digits it cannot keep |
| Splitting an amount without losing units | Instalments, pro-rata allocation | `allocate` (stage 2) |
| Interchange with columnar and SQL decimals | Polars, Parquet, databases | Same 38-digit range |

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
| **Plenty-native `Decimal`** (recommended) | Two `u64` limbs + scale + sign; 38 digits, scale 0–38 | Four inline fields; never allocates | Exact or error; rounding only on request | Plenty source in `stdlib/std/decimal.plenty` |

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

### Canonical form and equality

Plenty's `==` is structural and cannot be overridden
([operator boundaries](../design/language/04-expressions/operators.md#boundaries)),
and the [key proposal](user-type-hashing.md) derives hashing from fields with
no user `__hash__`. A scale-preserving decimal (GDA, Python, Java
`BigDecimal`) would therefore make `1.0 == 1.00` false, the well-known Java
`BigDecimal.equals` mistake, and could never be a correct dictionary key.

So `Decimal` stores the smallest scale that represents the value exactly
(`1.50` is stored as coefficient 15, scale 1), and zero has one
representation. Structural equality is numeric equality and structural hashing
is numeric hashing.

The cost: a value no longer remembers how many places it was written with.
`Decimal.parse("1.50")` displays as `1.5`. Financial output always formats to
a currency's places anyway, so formatting takes the places explicitly
(`price.places(2, Rounding.HalfEven)`). This is the main departure from
CPython and needs a decision (see [decisions](#decisions)).

### No context, no special values

A GDA context is hidden mutable state (Python stores it per thread), and its
default precision silently rounds results that do not fit. Instead, the
operation that can be inexact takes its scale and rounding mode as arguments,
and the others are exact. With no context there are no sticky flags to check,
and with errors as values there is no need for NaN or infinity. Negative zero
carries no financial meaning and would break canonical equality.

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

Overflow means a result with more than 38 digits or more than 38 fractional
digits; repeated multiplication without rounding eventually overflows instead
of losing digits. The same rule covers the methods that can overflow:
`div`, `round`, `mul_round` and `div_rem`, as it covers `pow` on integers.

`OverflowError[T]`'s `wrapped` field is the exact result modulo
2<sup>N</sup> for integers. For `Decimal` the recommended analogue is the
exact result truncated toward zero to 38 fractional digits, with its
coefficient reduced modulo 10<sup>38</sup>: deterministic, and like the
integer field, rarely useful except for diagnostics.

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
the call as `T`, contribute errors to enclosing blocks, and allow the
decorator in `std` before user code) and code generation (the `Err` branch to
the existing trap and the block's exit edge). Other library types could adopt
it later; `std.time.Duration`, which has `checked_*` methods returning
`RangeError`, is one candidate, out of scope here.

### Rounding modes

Seven modes, named by direction rather than by Python's ambiguous `ROUND_UP`
and `ROUND_DOWN`:

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
at every use. `NonZeroDecimal` mirrors `NonZero[T]`:

| `NonZero[T]` | `NonZeroDecimal` |
|---|---|
| A literal converts at compile time where expected; `0` is a compile error | `NonZeroDecimal.of(units: NonZero[i64], scale)` and `from_int(value: NonZero[i64])`: literal units convert at compile time, so `NonZeroDecimal.of(0, 2)` is a compile error |
| `NonZero.try_from(value) -> Result[NonZero[T], RangeError]`, `OutOfRange` for zero | `NonZeroDecimal.try_from(value: Decimal) -> Result[NonZeroDecimal, RangeError]`, `OutOfRange` for zero |
| `.get() -> T` | `.get() -> Decimal` |
| `*` and unary `-` keep the type; `+` and `-` return `T` | Same: `*`, unary `-` and `abs()` keep `NonZeroDecimal`; `+` and `-` return `Decimal` |
| Mixed `NonZero[T]` and `T` operands are a compile error suggesting `.get()` | Same, through the hooks' parameter types |
| Copy, `==`, printing, dictionary key | Copy, `==`, printing; keys with `Decimal` under B74 |

There is no flow narrowing (`if d != 0:` does not change `d`'s type), as for
integers. A runtime count converts once: `NonZeroDecimal.from_int(NonZero.try_from(n)?)`.
Canonical form keeps the guarantee cheap: zero has one representation, so
`try_from` tests one field. A non-zero product cannot round to zero, because
an exact product that needs more than 38 fractional digits overflows instead.

The division API:

```text
a.div(divisor: &NonZeroDecimal, places: i64, mode: Rounding) -> Decimal
a.div_rem(divisor: &NonZeroDecimal) -> (Decimal, Decimal)
```

- `div` returns the quotient correctly rounded to `places` fractional digits
  (0–38; others trap like `Decimal.of`) with `mode`. A quotient over 38 digits
  overflows under the `checked:` rules above; it has no other failure.
- `div_rem` returns an integral quotient and an exact remainder with the
  signs of integer `//` and `%`: the quotient floors and the remainder takes
  the divisor's sign.

Why still no `/`: a non-zero divisor removes the zero case but not rounding.
`1 / 3` has no exact result, so `/` would need fixed places and a fixed mode.
Polars rounds to the column scale and `rust_decimal` to 28 digits; either is
the silent rounding this design excludes, and a trap on every inexact
quotient would make `/` fail for most inputs. A `/` that takes its places from
the dividend's scale would make results depend on how a value was written,
which canonical storage does not record.

Integers differ today: `i64 // NonZero[i64]` is a compile error (the operand
must be `i64`), and integer `//` and `%` still trap on zero. Issue #584 left
`NonZero` divisors for `//` and `%` as separate work; `std.decimal` starts with
the rule that work would adopt.

### Parsing and formatting

`Decimal.parse(text)` follows the [numeric parsing contract](../design/facilities/01-builtins/numbers.md#parsing):
it allocates nothing, trims white space, and accepts an optional sign, digits,
an optional fraction, and an optional exponent (`1.5e3`). More than 38
significant digits, or more than 38 fractional digits after trailing zeros are
removed, is `ParseError.OutOfRange`; it never rounds. `inf`, `nan`, grouping
and locale forms are `ParseError.Invalid`.

`display` writes the canonical plain form (`-1234.5`, never an exponent).
Today an f-string spec on a class applies to its text: probing with
`std.complex`, `f"{Complex(1.25, 2.0):.3}"` prints `1.2`. For a decimal,
`{total:.2}` would therefore cut `19.999` to `19`. Until format specs reach
library types (stage 3), `Decimal` provides `places(n, mode)` and the
reference must warn against specs.

## Fit with the language today

| Need | Today | Consequence |
|---|---|---|
| 128-bit coefficient | No `u128`/`i128`; `u64` has wrapping and checked operations but no high product | Store two `u64` limbs; add `u64.widening_mul` (Cranelift `umulhi`, available on x86-64 and AArch64 in the pinned 0.137); 32-bit-half multiplication works meanwhile at about four times the multiplies |
| Operators | [Operator hooks](../design/language/04-expressions/operators.md) as in `std.complex` | `+ - *` and unary `-` |
| Ordering `<` | Not available for classes ([B37](../backlog.md), [ordering proposal](user-type-ordering.md)) | `compare()` until `__cmp__` lands |
| Dictionary keys | Not available for classes ([B74](../backlog.md)) | Automatic once structural class keys land, because of canonical form |
| Literals and constants | `const` accepts only scalars, `str`, `bytes` | `Decimal.of(1999, 2)` is allocation-free and infallible; `parse` for text |
| Format specs | Applied to display text | `places()`; later a spec hook |
| `checked:` and traps | Integer operators, `pow`, `@newtype` forwarding; hooks keep trapping; std cannot raise the arithmetic trap | `@arithmetic` methods and hooks (stage 1) |
| Non-zero divisor | `NonZero[T]` for integer `T` only | `NonZeroDecimal` in `std.decimal`; no compiler change |

Decimal needs `widening_mul` on its own account: every product of two 128-bit
coefficients, every scaling by a power of ten, and the 256-by-128-bit division
behind `div` and `round` is built from 64×64→128-bit products.

## API sketch

```text
from std.decimal import Decimal, NonZeroDecimal, Rounding

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
    # private: _high: u64, _low: u64, _scale: u8, _negative: bool

    # Construction: allocation-free.
    Decimal.ZERO, Decimal.ONE
    Decimal.of(units: i64, scale: i64) -> Decimal        # of(1999, 2) is 19.99; traps outside 0..38
    Decimal.from_int(value: i64) -> Decimal
    Decimal.parse(text: str) -> Result[Decimal, ParseError]
    Decimal.from_f64(value: f64, places: i64, mode: Rounding) -> Result[Decimal, RangeError]   # non-finite or too large
    Decimal.from_parts(high: u64, low: u64, scale: i64, negative: bool) -> Result[Decimal, RangeError]

    # Exact arithmetic. @arithmetic: traps on overflow; Err(OverflowError[Decimal]) inside checked:.
    a + b, a - b, a * b, -a
    a.abs() -> Decimal

    # Rounding is always explicit. Also @arithmetic.
    a.round(places: i64, mode: Rounding) -> Decimal
    a.div(divisor: &NonZeroDecimal, places: i64, mode: Rounding) -> Decimal
    a.mul_round(&b, places: i64, mode: Rounding) -> Decimal   # exact product, then round
    a.div_rem(divisor: &NonZeroDecimal) -> (Decimal, Decimal) # floored quotient, exact remainder

    # Inspection and conversion.
    a.compare(&b) -> i64              # -1, 0, 1; `<` after B37
    a.sign() -> i64
    a.scale() -> i64                  # canonical scale
    a.is_integer() -> bool
    a.to_i64(mode: Rounding) -> Result[i64, RangeError]
    a.to_f64() -> f64                 # nearest f64
    a.to_units(scale: i64) -> Result[(u64, u64, bool), RangeError]   # unscaled value at a fixed scale; fails if not exact

    # Text.
    a.display(out)                    # canonical plain form
    a.places(n: i64, mode: Rounding) -> Places   # copyable view; displays exactly n places

@copy
pub class NonZeroDecimal:
    NonZeroDecimal.of(units: NonZero[i64], scale: i64) -> NonZeroDecimal
    NonZeroDecimal.from_int(value: NonZero[i64]) -> NonZeroDecimal
    NonZeroDecimal.try_from(value: Decimal) -> Result[NonZeroDecimal, RangeError]
    d.get() -> Decimal
    d * e, -d, d.abs() -> NonZeroDecimal
    d + e, d - e -> Decimal
```

Conversions report `RangeError.OutOfRange`, as integer `try_from` does; there
is no `DecimalError`. No arithmetic returns `DivisionByZero`, since no
operation accepts a zero divisor.

Example, in the proposed API:

```text
from std.decimal import Decimal, NonZeroDecimal, Rounding
from std.fmt import FormatError

def unit_price(total: Decimal, count: i64) -> Result[Decimal, RangeError]:
    per = NonZeroDecimal.from_int(NonZero.try_from(count)?)
    Ok(total.div(&per, 2, Rounding.HalfEven))

def main() -> Result[(), IoError | ParseError | RangeError | FormatError]:
    price = Decimal.parse("19.99")?
    rate = Decimal.parse("0.0825")?
    subtotal = price * Decimal.from_int(3)
    tax = (subtotal * rate).round(2, Rounding.HalfEven)
    total = subtotal + tax
    print(f"{total.places(2, Rounding.HalfEven)}"?)?           # 64.92
    print(f"{unit_price(total, 7)?.places(2, Rounding.HalfEven)}"?)?   # 9.27
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
  A column `Decimal(p, s)` reads as canonical `Decimal` values; writing to a
  column scales each value to `s` exactly or fails with `Inexact` unless the
  caller rounds first. Column arithmetic stays in Polars, with its own
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
   - `@arithmetic` methods and operator hooks
     ([gap](#gap-library-types-in-the-trap-rules)), with tests on a small
     test-only class: trap position and caller note, `TrapKind.Arithmetic`,
     `checked:` error types, and the lexical rule.
2. **Core module.** `Decimal`, `NonZeroDecimal`, `Rounding`; parsing,
   display, `places`; exact `+ - *` under the trap and `checked:` rules;
   `round`, `div`, `mul_round`, `div_rem`, `compare`, conversions;
   `allocate(parts)` for splitting an amount into parts that sum exactly.
   Reference page, a tutorial lesson replacing `f64` money arithmetic,
   differential tests against CPython, allocation tests proving no
   allocation.
3. **Language integration.** `__cmp__` ordering when B37 lands; dictionary
   keys when B74 covers classes; a format-spec hook so `{total:.2f}` rounds
   (half-even, as Python's `format` does) instead of truncating text.
4. **Interchange.** Polars decimal columns (B76), JSON number text, CSV
   helpers.
5. **Only on evidence.** A standalone arbitrary-precision decimal design, or a
   fallible libmpdec wrapper for the full GDA operation set; a decimal literal
   suffix (`19.99d`) if `parse` and `of` prove too noisy.

## Decisions

Decided in review (2026-10-31):

1. **Operator overflow** traps like integers and becomes
   `Err(OverflowError[Decimal])` inside `checked:`. No `checked_*` methods,
   contexts or signals; library types join the existing rules through
   `@arithmetic`.
2. **Division by zero** is ruled out by a `NonZeroDecimal` divisor that
   mirrors `NonZero[T]`. Division is `div(divisor, places, mode)`; there is
   no `/`.
3. **No coupling to other numeric proposals.** `widening_mul` is justified by
   decimal's own products; arbitrary precision is a separate later design.

Open:

1. **Canonical vs scale-preserving values.** Canonical (recommended) makes
   `==` and future keys numeric but drops trailing zeros (`1.50` → `1.5`).
   Scale-preserving matches Python and Polars columns but makes `1.0 == 1.00`
   false under structural equality, until or unless Plenty allows custom
   equality.
2. **Digit limit.** 38 digits (recommended; Arrow/SQL), 34 (IEEE
   `decimal128`), or 28 (`rust_decimal`).
3. **Rounding mode.** Always explicit (recommended) or a default of
   `HalfEven`.
4. **Multiply prerequisite.** `u64.widening_mul` (recommended), a full
   `u128`/`i128` type (larger change; also serves `std.uuid` and
   `std.encoding`), or 32-bit halves in Plenty only.
5. **Non-zero spelling.** `NonZeroDecimal` in `std.decimal` (recommended; no
   compiler change) or `NonZero[Decimal]`, which would need `NonZero`'s
   `IntType` bound widened to library types that define zero.
6. **`wrapped` for decimals.** Truncate to 38 fractional digits and reduce the
   coefficient modulo 10<sup>38</sup> (recommended), or another deterministic
   rule.
7. **`@arithmetic` scope.** Standard library only at first (recommended), or
   user code from the start.
