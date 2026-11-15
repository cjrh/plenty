# `std.bigint` — arbitrary-precision integers

`BigInt` is a signed integer with no fixed bit width. Import it from
`std.bigint`; constructors are `from_i64`, `from_u64`, and `parse`.
It is implemented directly in Plenty, with fallible ordinary list allocations.
There is no native dependency or engine.

```plenty
from std.bigint import from_i64, parse, BigIntError
from std.fmt import FormatError

def main() -> Result[(), AllocError | BigIntError | IoError | FormatError]:
    large = parse("18446744073709551616")?
    factor = from_i64(-7)
    print((large * factor)?)?
    print((large // factor)?, (large % factor)?)?
    print(large.to_string(16)?)?
    Ok(())
```
```output
-129127208515966861312
-2635249153387078803 -5
10000000000000000
```

## Construction and conversion

| API | Contract |
|---|---|
| `from_i64(value: i64) -> BigInt` | Inline value; no allocation. |
| `from_u64(value: u64) -> Result[BigInt, AllocError]` | Allocates only above `i64.MAX`. |
| `parse(text: &strview, radix: i64 = 10, max_digits: NonNegative[i64] = 10000) -> Result[BigInt, BigIntError]` | ASCII digits in radix 2–36, optional leading `+` or `-`; upper and lower case accepted. No whitespace, separators, or radix prefixes. The digit cap excludes the sign and includes leading zeros; checked before allocating arithmetic storage. |
| `BigInt.to_i64() -> Result[i64, BigIntError]` | Exact conversion; `OutOfRange` if it does not fit. |
| `BigInt.to_u64() -> Result[u64, BigIntError]` | Exact conversion; negative values and values above `u64.MAX` give `OutOfRange`. |
| `BigInt.to_f64() -> f64` | Nearest representable `f64`, ties to even; overflow gives signed infinity. Does not allocate. |
| `BigInt.bit_length() -> i64` | Number of bits in the absolute value; zero gives zero. Does not allocate. |
| `BigInt.to_string(radix: i64 = 10, max_digits: NonNegative[i64] = 10000) -> Result[str, BigIntError]` | Lower-case digits, no prefix or leading zeros; negative values start with `-`. The cap excludes that sign. |

`BigIntError` is `InvalidDigit`, `InvalidRadix`, `LimitExceeded`, `OutOfRange`,
`ZeroDivision`, or `Allocation(AllocError)`. Invalid text includes an empty
string or a sign without digits. Zero is always positive, including when parsed
from `-0`. Constructors and text conversions retain no input borrow.

## Arithmetic and ownership

Operators borrow both operands and return new owned values:

| Operation | Result |
|---|---|
| `a + b`, `a - b`, `a * b`, `-a` | `Result[BigInt, AllocError]` |
| `a // b`, `a % b` | `Result[BigInt, AllocError]`; zero divisor traps, naming the operation. |
| `n + a` where `n: i64` | `Result[BigInt, AllocError]` through `__radd__`. For `a + n`, write `a + from_i64(n)`. |
| `a << count`, `a >> count` where `count: i64` | `Result[BigInt, AllocError]`; negative counts trap. Right shift rounds toward negative infinity. |
| `a & b`, `a \| b`, `a ^ b` | `Result[BigInt, AllocError]`; infinite signed two's-complement semantics. |
| `a.checked_divmod(b)` | `Result[(BigInt, BigInt), BigIntError]`; zero gives `ZeroDivision`. |
| `a.add_assign(b)` | `Result[(), AllocError]`; `a` stays unchanged on failure. |
| `a.cmp(b)` | `i64`: `-1`, `0`, or `1`; no allocation. |

Division uses floor semantics: `a == q * b + r`, with `r` zero or having the
same sign as `b`, and `abs(r) < abs(b)`. There is no arithmetic overflow.
Allocation failure, including an unrepresentable capacity, reaches the caller.
An operator whose operands and result fit `i64` does not allocate. Results
crossing that boundary can allocate scratch even when the final value is small.
Use `(a + b)?` to propagate failure. In-place operator hooks cannot report
allocation failure, so `+=` is unsupported; use `add_assign` or explicit assignment.

`BigInt` is not a copy type. `copy(value)?` duplicates its limbs. Equality is
structural and exact because storage is canonical: values in the `i64` range
are inline; larger values use unsigned 64-bit magnitude limbs without high
zeros and a separate sign. The representation is private. Ordering operators
and dictionary/set keys are not supported; use `cmp` for ordering.

## Allocation, limits, and algorithms

Addition/subtraction allocate proportional to the operand lengths;
multiplication uses schoolbook arithmetic and balanced Karatsuba from 32 limbs.
Toom-3 handles medium products from 128 limbs when both operands have nonempty
thirds and their size ratio is at most 5:4. Its five evaluations are at 0, ±1,
2, and infinity. Large, balanced products can use exact five-prime
number-theoretic transforms (NTTs) with 128-bit digits (two `u64` limbs),
Garner CRT coefficient recovery, and base-2¹²⁸ carries. Five independently
generated, approximately 60-bit Proth primes provide exact convolution;
primality witnesses are retained in
`experiments/bigint-comparison/algorithms-next/constants.py`. CRT reconstructs
each coefficient in five scalar `u64` words. All residue, root and product
arithmetic uses exact Montgomery reduction with R = 2⁶⁴. The scalar radix-2
transforms reuse fixed scratch and computed twiddle tables. At 65,536 bits per
operand, the transform length is 1,024, compared with 8,192 for the former
16-bit path and 4,096 for the intermediate 32-bit path.
The NTT requires both operands to have at least 1,024 limbs, a size ratio at
most 2:1, at least seven-eighths transform occupancy, and at most 524,288
combined limbs. Occupancy uses the actual digit counts, rounding each operand's
limb count up to a pair, including padding for odd limb counts. The primes'
2⁴⁰ transform order exceeds this retained cap. Other shapes use Toom-3,
Karatsuba, or schoolbook multiplication as applicable. This transform bound
is an algorithm-selection limit, not a limit on BigInt precision.
Division uses Knuth's Algorithm D with temporary base-2³² digits so trial
quotients fit `u64`. Karatsuba and Toom-3 borrow operand limb views and write
their low and high products (the evaluations at 0 and infinity) directly into
disjoint regions of the final output, avoiding two owned recursive product
arrays. Evaluation and interpolation scratch remains fallibly allocated.
Division holds views of its fixed scratch buffers while computing quotient
digits. Parsing and rendering split digit ranges recursively, sharing binary
and exact radix powers within one conversion. Single-word rendering uses scalar
division without a power cache. All their scratch allocations are fallible.
Truncated vector transforms and specialized squaring are not implemented.
No native multiplication kernel is used, and crossover choices are implementation
details, not performance promises.

Text conversion has explicit digit caps. Formatting through `display[W](out:
&mut Formatter[W]) -> Result[(), FormatError]` uses decimal with the default
10,000-digit cap; exhaustion becomes `FormatError.LimitExceeded`, and allocation
failure becomes `FormatError.Allocation`. Larger intentional output uses
`to_string(max_digits=...)`. Formatter output has its own byte limit.

Arithmetic is bounded by the existing operand sizes and requested shift count,
rather than a global precision limit. Callers processing untrusted arithmetic
must enforce their own operation and bit budgets; repeated multiplication or a
huge shift can request large memory. A zero left shift operand returns zero
without allocating, even for a huge count.

The compiler provides two allocation-free `u64` limb methods:
`a.widening_mul(b) -> (u64, u64)` returns the low and high halves of the exact
128-bit product; `a.carrying_add(b, carry: bool) -> (u64, bool)` returns the
wrapped sum and carry-out for `a + b + carry`. The required boolean here is a
numeric carry bit, not a mode selection.

## Acknowledgements

- Donald Knuth, *The Art of Computer Programming*, volume 2,
  [Seminumerical Algorithms](https://www-cs-faculty.stanford.edu/~knuth/taocp.html),
  section 4.3.1: schoolbook arithmetic and normalized long division (Algorithm D).
- Donald Knuth, *The Art of Computer Programming*, volume 2,
  [Seminumerical Algorithms](https://www-cs-faculty.stanford.edu/~knuth/taocp.html),
  section 4.3.3: Karatsuba's three-product and Toom-3's five-evaluation
  multiplication algorithms. These are
  direct Plenty implementations of published algorithms; no third-party code
  or data ships.
- [FLINT: Multiplication with several primes and CRT](https://flintlib.org/doc/algorithms/fft/multimodular.html):
  the published multimodular convolution method used for large multiplication,
  also used by Malachite’s FLINT-derived small-prime FFT. This is an independent
  integer radix-2 implementation; no FLINT/Malachite code or tables ship.
- Peter L. Montgomery, [Modular multiplication without trial division](https://doi.org/10.1090/S0025-5718-1985-0777282-X):
  exact modular arithmetic for transform residues, roots and products.
