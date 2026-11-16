# Rust to Plenty

Read the skill's cheat sheet and signature index before writing the port.
Both languages track moves and borrows, but Rust syntax, traits, lifetimes and
library methods do not transfer mechanically.

## A practical sequence

1. Record the Rust input/output contract: widths, overflow policy, errors,
   mutations, ownership, text versus bytes, and cleanup. Keep the source build
   profile/features fixed when comparing behavior.
2. Map owned buffers to `list[T]` or `std.bytes.Bytes`, borrowed slices to
   `&[T]` / `&mut [T]`, records to classes, and tagged alternatives to enums.
   Choose explicit integer widths; `usize` is not a Plenty type, and collection
   lengths use `i64`. Preserve the application's domain rather than casting
   everything until it type-checks.
3. Port one scalar operation at a time, keeping explicit error and ownership
   boundaries. Use a simple loop before translating an elaborate iterator chain.
   Look up standard-library capabilities before recreating a Rust helper.
4. Type-check and run the port, comparing results and failure behavior with
   Rust on empty inputs, negative values, high-bit bytes and width boundaries.

## Common replacements

| Rust | Plenty |
|---|---|
| `fn f(x: i64) -> T { ... }` | `def f(x: i64) -> T:` with an indented body. A final expression returns its value. Use `()` for unit. |
| `let`, `let mut` | `name = value`, `mut name = value`. Later assignment does not redeclare the binding. |
| `Vec<T>`, `&[T]`, `&mut [T]` | `list[T]`, `&[T]`, `&mut [T]`. Borrow owners explicitly for clarity; mutable borrows require `&mut`. |
| `String`, `&str`, `&[u8]` | Owning immutable `str`, read-only `&strview`, and `&[u8]`. Use `Bytes` for mutable octets; do not translate a mutable Rust `String` into mutable `str`. |
| `Option<T>` / `None` | `Option[T]` / `Nothing`; `Some(value)` is unchanged. |
| `Result<T, E>` / `?` | `Result[T, E]` / `?`. Explicit `Ok(value)` on success; union error types can propagate their members, but custom enum wrappers require `.map_error(...)`. |
| `#[derive(Copy)]` | `@copy` on a class or enum with copyable fields/payloads. Ordinary classes and enums move by default; numbers, `str` and `bytes` copy. |
| `.clone()` | `copy(value)?` for eligible owned mutable data; copying a `str` needs only ordinary value use. Use `.share()` for a `Shared` handle. |
| `impl`, traits and closures | Put methods in the class/enum; use a protocol for a supported generic interface and typed inline `def(...) -> T: ...` for a closure. There is no general `dyn Trait` or Rust lifetime syntax; consult the skill's boxed-protocol and borrowing references before designing stored polymorphism or borrowed records. |
| `.iter().map(f)` | `iter(&values).map(f)` or a loop. Borrowed iteration yields references: read elements with `*value`. Look up pipeline callback shapes rather than assuming Rust's adapter signatures. |
| `.map_err(f)` | `.map_error(f)`; `?` does not implicitly invoke Rust-style `From` conversions. |

Rust allocations often abort on failure behind an infallible API. Plenty list
construction, `append`, copies and string building return `Result`; include
`AllocError` and handle it. Printing also returns a `Result`. A Rust destructor
or `Drop` implementation is not automatically equivalent to Plenty cleanup:
check resource/error contracts before translating it to `__del__` or scope exit.

Plenty borrowing has fewer lifetime shapes than Rust. Read
[ownership](ownership.md) before porting returned references, self-referential
data or records that borrow multiple origins. Keep owners alive and redesign
unsupported storage instead of inserting unsafe pointer conversions.

## Preserve integer and text semantics

- Ordinary integer arithmetic can return `OverflowError[T]` from the enclosing
  function. `unchecked:` traps on violated arithmetic preconditions; it does
  not reproduce Rust release-mode wrapping. Use `.wrapping_add`, `.wrapping_sub`
  and `.wrapping_mul` for deliberate modulo arithmetic at any supported width.
- Integer `T(value)` casts require representability. Use `T.try_from(value)?`
  for recoverable conversion and `T.wrapping_from(value)` for low-bit integer
  conversion. Do not use a checked cast to reproduce a narrowing Rust `as`.
  Check float-to-integer conversion separately; Rust and Plenty differ on
  out-of-range values and NaN.
- Signed Rust `/` truncates toward zero; Plenty `//` floors, and the remainder
  sign changes correspondingly. For a truncating port, use the verified
  [division helper](c-to-plenty.md#example-preserve-c-signed-division), including
  its zero and signed-minimum overflow errors. Never route integer division
  through floating point.
- Rust string lengths and offsets are UTF-8 bytes; Plenty text lengths/indexes
  count Unicode scalars. Use `.as_bytes()` when porting a byte-oriented parser.
  Check indexes explicitly if negative positions must be rejected: Plenty
  negative indexing reads from the end.

## Example: a borrowed byte checksum with explicit wrapping

Rust:

```rust
fn checksum(data: &[u8]) -> u32 {
    let mut value = 0u32;
    for &byte in data {
        value = value.wrapping_mul(33).wrapping_add(u32::from(byte));
    }
    value
}

fn main() {
    println!("{}", checksum(b"abc"));
}
```

Plenty:

```plenty
def checksum(data: &[u8]) -> u32:
    mut value = 0u32
    for byte in data:
        value = value.wrapping_mul(33u32).wrapping_add(u32(*byte))
    value

def main() -> Result[(), IoError]:
    print(checksum(b"abc"))?
    Ok(())
```
```output
108966
```

Both print `108966`. Neither checksum allocates or consumes the input. This is
a porting example, not a cryptographic checksum.
