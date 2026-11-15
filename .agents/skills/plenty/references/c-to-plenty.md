# C to Plenty

Start with the algorithm's defined input domain and observable behavior.
Translate the data model before translating statements. Use the skill's cheat
sheet and signature index to verify Plenty APIs.

## A practical sequence

1. Identify exact integer widths/signedness, pointer lengths and aliasing,
   allocation ownership, failure conventions, and external ABI requirements.
2. Replace pointer-plus-length inputs with `&[T]` and exclusive outputs with
   `&mut [T]`. Use `list[T]` for owned growable storage, `bytes` for immutable
   octets, or `std.bytes.Bytes` for a mutable byte buffer. A view borrows existing
   storage; constructing an owner is a separate fallible operation.
3. Use classes for records and enums for tagged unions. Classes do not promise
   a C struct's layout. Move cleanup into owned resources and scope exit;
   preserve close/flush errors that matter to the application.
4. Turn status codes, sentinel pointers and `errno` handling into a meaningful
   `Result` or `Option`. Preserve the original cause and distinguish EOF,
   absence and failure. Do not carry an invalid pointer representation into
   ordinary Plenty code.
5. Compare defined C cases with the port: empty buffers, high-bit bytes,
   negative division, conversion limits and deliberate overlap cases. A C
   sanitizer run helps find invalid oracle inputs. Exclude undefined behavior
   from equivalence claims rather than copying an accidental machine result.

## Integer semantics need an explicit decision

| C behavior | Plenty decision |
|---|---|
| Widths of `int`, `long`, `size_t`, signedness of `char` depend on target | Select `i32`, `i64`, `u64`, `u8`, etc. from the data contract; do not mechanically map every integer to `i64`. Views expose their length without a separate `size_t`. |
| Unsigned arithmetic wraps; narrow operands undergo integer promotions | Use matching-width `.wrapping_add`, `.wrapping_sub` and `.wrapping_mul` for intended modulo arithmetic, including `u64`. Preserve C promotions when an intermediate must be wider than the stored result. |
| Signed overflow is undefined | Establish valid ranges or return an error. `unchecked:` traps on violation and is not a wrapping mode. |
| Signed `/` truncates toward zero, `%` follows the dividend | Plenty `//` floors and `%` follows the divisor. Adapt both if negatives are permitted; see helper below. |
| Integer conversions may truncate/reinterpret | `T(value)` requires representability and traps otherwise; `T.try_from(value)?` recovers; `T.wrapping_from(value)` deliberately preserves low integer bits. |
| Out-of-bounds pointer/index operations are undefined | Plenty indexing traps; `.get()` returns `Option`. Plenty negative indexes address from the end, so validate a C index whose negative values should be rejected. |
| Shift edge cases depend on signedness and valid counts | Verify both count and value semantics; do not assume a compiler's masked shift count is a language guarantee. |

For an 8-bit modulo sum, use `a.wrapping_add(b)` for `a: u8`, `b: u8`.
Use `T.wrapping_from(value)` when the conversion itself must keep low bits.
Do not force every narrow C intermediate to wrap: an expression evaluated
after integer promotion may need a wider Plenty type before the final
conversion. Casting after an overflowing checked operation would be too late.

## Example: preserve C signed division

```plenty
def trunc_divrem(a: i64, b: i64) -> Result[(i64, i64), OverflowError[i64] | ZeroDivisionError]:
    q = a // b
    r = a % b
    if r != 0 and ((a < 0) != (b < 0)):
        return Ok((q + 1, r - b))
    Ok((q, r))

def main() -> Result[(), IoError | OverflowError[i64] | ZeroDivisionError]:
    print(trunc_divrem(-7, 3)?)?
    print(trunc_divrem(7, -3)?)?
    Ok(())
```

This prints `(-2, -1)` and `(-2, 1)`. Zero division and `i64.MIN / -1`
return errors, rather than reproducing C's undefined cases.

## Text, buffers and ABI boundaries

A C `char *` might mean encoded text, arbitrary bytes, or a NUL-terminated
buffer. Decide which before choosing `str`/`&strview` or `&[u8]`; text indexing
uses Unicode scalar positions. A borrowed view's owner must stay alive. Do not
return a view into local scratch storage or manufacture overlapping mutable
views just because C accepts aliased pointers.

When the task needs C interoperability rather than a source port, read
[raw host interfaces](../../../../book/src/design/facilities/04-interop/raw-host-interfaces.md).
Trusted `.plentyi` declarations and opaque pointers are ABI tools, not a reason
to implement an otherwise straightforward algorithm through FFI. Never infer
C struct layout from an ordinary Plenty class.
