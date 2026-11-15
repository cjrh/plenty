# Rust to Python

Use this for a Python port or a small reference implementation used to check a
Plenty port. Translate behavior before syntax. A Python oracle that silently
changes division or overflow can agree with an incorrect port.

## A practical sequence

1. Identify input/output types, valid input ranges, errors, mutations, byte
   order, and whether overflow is checked, wrapping, or excluded by a proof.
2. Implement the scalar algorithm using Python's ordinary functions, lists,
   dictionaries and explicit loops. Replace Rust iterators with a loop or a
   generator expression; retain ordering and short-circuit behavior.
3. Compare both implementations on empty/singleton inputs, boundary values,
   negative operands and representative random cases. Compare returned values,
   errors and externally visible mutation. Keep the Rust profile/features fixed.

## Differences that change results

| Rust | Python conversion |
|---|---|
| Fixed-width integers | `int` is unbounded. Preserve range checks, or mask after each operation whose wrapping is observable: `x & ((1 << bits) - 1)`. Decode a signed two's-complement result explicitly. |
| Signed integer `/` and `%` | Rust truncates toward zero; Python `//` floors and `%` follows the divisor's sign. Use the helper below. Never use `int(a / b)`: float conversion loses large integer precision. |
| `wrapping_*`, `checked_*`, `saturating_*` | Implement their distinct contracts explicitly. Ordinary Rust overflow depends on configuration; do not infer the contract from one successful run. |
| `as` integer casts | Python `int(x)` does not reproduce integer narrowing or sign reinterpretation. Model the source/target widths. Check float-cast behavior separately. |
| `Vec<T>` assignment/move, `&mut T` | Python assignment aliases. Choose mutation or an independent copy deliberately; `list(x)` copies only the outer list. |
| `Option<T>` | `None` works only if `Some(None)` need not be distinguished. Otherwise use a tagged value or sentinel. |
| `Result<T, E>` / `?` | Choose explicit tagged results or domain exceptions. Preserve error cases; catch only expected exceptions rather than returning a fabricated success. |
| `String`, `&str`, `&[u8]` | Use `str` for text, `bytes` for immutable octets, `bytearray` for mutable octets. Rust string length is bytes; Python string length is code points. UTF-8 byte offsets require encoding. |
| `HashMap` traversal | Python dicts retain insertion order; do not turn unspecified source order into an accidental output contract. Sort when the application requires canonical output. |
| `Drop` / RAII | Use `with` or `try/finally` for prompt resource cleanup. Do not rely on Python object finalization. |

Python negative list indexes wrap from the end, and slices clamp bounds. Rust
indexing rejects out-of-range positions. Add explicit checks if the port must
preserve those failures. A Python `assert` is unsuitable for required validation
because optimized execution can remove it.

## Exact truncating division

```python
def trunc_divrem(a: int, b: int) -> tuple[int, int]:
    if b == 0:
        raise ZeroDivisionError("division by zero")
    magnitude = abs(a) // abs(b)
    q = -magnitude if (a < 0) != (b < 0) else magnitude
    return q, a - q * b

assert trunc_divrem(-7, 3) == (-2, -1)
assert trunc_divrem(7, -3) == (-2, 1)
assert trunc_divrem(10**100 + 1, 3)[1] == 2
```

This models mathematical truncation. For a fixed signed Rust type, also reject
`MIN / -1` (and the corresponding remainder operation), which overflows.
Do not make Python speed or memory measurements stand in for a native port's
performance; use it as an independent correctness oracle.
