# Ownership and text adapters

Raw C signatures do not prove lifetime or ownership rules. A trusted `.plentyi`
module expresses the application-facing contract through ordinary Plenty classes,
references, and `Result`/`Option`; its private declarations describe native calls.
Bindings must document the promises made by the C implementation.

## Borrowed arguments

An `extern def` parameter may use `&T` or `&mut T` when `T` is a supported scalar
or opaque pointer. This passes `const T *` or `T *`, respectively, to initialized
storage for the duration of the call. A borrowed opaque pointer passes a pointer
to the pointer, suitable for C output parameters. Mutable arguments obey ordinary
exclusive borrowing. Native code must not retain these addresses or write
through an immutable borrow. It may access only the declared scalar's bytes;
Plenty slot padding and surrounding storage are private.

Text parameters require an explicit representation adapter:

```text
extern def inspect(text: &str as utf8) -> u64 = "inspect_bytes"
extern def length(text: &str as c_string) -> Result[u64, CStrError] = "strlen"
```

| Parameter | Actual C arguments | Allocation |
| --- | --- | --- |
| `text: &str as utf8` | `const uint8_t *data, size_t byte_len`, in that order | None; lends the existing bytes, including embedded NULs |
| `text: &str as c_string` | `const char *data` | One temporary terminated copy; rejects any embedded NUL |

These are read-only, call-scoped buffers. Neither permits retention, mutation,
or a returned pointer into the argument. Empty UTF-8 views have length zero and
must not be dereferenced. Normal strings keep their counted UTF-8 representation.

Any declaration using `c_string` must return `Result[T, CStrError]`, where `T`
is a supported C scalar/pointer or `()` for C `void`. Its **C return type is T**;
the generated Plenty adapter returns `Ok` around that value. Conversion happens
left to right before the C call. On embedded NUL or allocation failure, it skips
the call, releases earlier temporary buffers, and returns `Err`. Buffers are also
released when the C call returns. `CStrError` is an allocation-free builtin with
`EmbeddedNul` and `Allocation(AllocError)` variants. It describes conversion
failure, not a C function's status code: native errors still need an explicit
wrapper that interprets the returned C value.

Other native strings are not implicitly decoded. Returned pointers must have an
independent lifetime under the binding's contract. General mutable byte buffers,
retained/returned buffer loans, and copying arbitrary C bytes into `str` are not
part of this subset.

## Owned handles and transfer

An ordinary public class can own a private opaque pointer and call the matching
foreign destroy function in `__del__`. It then moves, cannot be copied, and drops
on scope exit or `?`, like other resources. Borrowing a class does not transfer
its native ownership. The binding author chooses immutable or mutable methods
according to the C library's aliasing and mutation guarantees, not merely its
use of `const`.

For fallible acquisition, first construct an owner initialized with null, then
acquire into its pointer in a public factory returning `Result`. The fully
initialized owner has an active destructor before any native resource exists.
This avoids leaks when native acquisition partially succeeds or later work fails.
The destructor must guard null, unless the C destroy operation explicitly accepts
it. It always uses the matching native release function, never Plenty's allocator.
Class storage allocation remains fallible and happens before acquisition. Native
allocation failures are reported only when the library exposes a recoverable error.

Transfer policies are explicit wrapper code:

- If C consumes unconditionally, move the Plenty owner into the wrapper, take its
  raw pointer, and clear the owner's field before calling C, on both result paths.
- If C consumes only on success, clear the field on success; on failure return
  the intact owner, for example as `Err(owner)` in `Result[(), Handle]`.
- For a borrowed call, retain ownership and select an appropriate `&Handle` or
  `&mut Handle` interface. The library may not retain borrowed addresses.

C fixtures verify matching destruction, partial acquisition, subsequent failure,
both transfer policies, borrow rejection, output scalars, UTF-8 extent, NUL
rejection, and failures at each temporary-string allocation. The runtime's C-string
conversion is also exercised independently under Miri.
