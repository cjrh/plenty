# Borrowed arguments and text adapters

Foreign borrowing is call-scoped. An `extern def` parameter `&T` or `&mut T`,
where T is a supported scalar or opaque pointer, becomes C `const T *` or `T *`
to initialized storage. Borrowing an opaque pointer passes a pointer to the
pointer, suitable for an output parameter.

Native code must not retain these addresses or write through a shared borrow.
Mutable arguments obey exclusive borrowing; native code may access exactly the
declared scalar's bytes, with no assumptions about Plenty slot padding.

## Text representations

```text
extern def inspect(text: &str as utf8) -> u64 = "inspect_bytes"
extern def length(text: &str as c_string) -> Result[u64, CStrError] = "strlen"
```

| Adapter | Actual C arguments | Construction |
| --- | --- | --- |
| `&str as utf8` | `const uint8_t *data, size_t byte_len` | No allocation; existing bytes including NUL |
| `&str as c_string` | `const char *data` | One temporary terminated copy; rejects embedded NUL |

Both buffers are read-only and call-scoped: C cannot mutate, retain, or return
an address into them. Empty UTF-8 views have length zero and must not be
dereferenced. Ordinary strings retain their counted UTF-8 representation.

Any declaration with `c_string` must return `Result[T, CStrError]`, with T a
supported C scalar/pointer or unit. The actual C return type is T; the adapter
wraps it in Ok. Conversion runs left to right before native entry. Embedded NUL
or allocation failure skips the call, releases earlier buffers, and returns Err;
normal return also releases all temporary buffers.

`CStrError` has allocation-free `EmbeddedNul` and `Allocation(AllocError)`
variants. It reports conversion failure rather than native status: wrappers must
interpret native errors separately.

Returned native strings are not implicitly decoded. Returned pointers need an
independent lifetime. Retained/returned buffer loans and copying arbitrary C
bytes directly into str are unsupported.

## Strided numeric lists

`&list[T] as strided` and `&mut list[T] as strided`, for primitive numeric
`T`, pass three C arguments: a data pointer, `size_t len`, and
`size_t stride_bytes`. Element `i` lives at `data + i * stride_bytes`.
The pointer may be null for an empty list and must then not be dereferenced.
The stride describes Plenty storage; it is not necessarily `sizeof(T)`.

The adapter does not allocate or pack a temporary collection. Native code
may read only each declared scalar's bytes and may write those bytes only
through a mutable loan. It must preserve padding and runtime metadata,
cannot resize the list, and cannot retain or return the pointer. Use
`memcpy` or unaligned scalar accesses if the native type requires alignment.
No booleans, strings, objects, nested lists or nullable elements are admitted.
Exclusive borrowing rules still apply. Current numeric lists store naturally
sized elements; generated and runtime reads sign-extend narrow signed values
directly, so mutable views need no post-call normalization pass.
The optional [dataframe engine](../../stdlib/dataframe.md) uses this adapter
for bulk transfer.
