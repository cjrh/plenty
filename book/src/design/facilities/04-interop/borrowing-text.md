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
| `&list[str] as c_strings` | `const char *const *values` | One temporary pointer table and terminated UTF-8 copies; rejects embedded NUL |

Text buffers are read-only and call-scoped: C cannot mutate, retain, or return
an address into them. Empty UTF-8 views have length zero and must not be
dereferenced. Ordinary strings retain their counted UTF-8 representation.

Any declaration with `c_string` or `c_strings` must return `Result[T, CStrError]`, with T a
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

## Strided numeric lists and byte views

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

`&[u8] as strided` uses the same three C arguments with a byte stride of 1.
It borrows a literal, a `Bytes` buffer, a `list[u8]`, or a subview without
allocation or copying. The native call may read exactly `len` bytes and must
not retain the pointer. Empty views must not be dereferenced. Mutable byte
views and views of other element types are not admitted by this adapter;
mutable numeric lists retain the contract above.

## C string vectors

```text
extern def inspect(arguments: &list[str] as c_strings) -> Result[i32, CStrError] = "inspect_arguments"
```

`c_strings` packs a pointer table followed by the strings' UTF-8 bytes in one
fallible allocation. Every string has a trailing NUL; the table has a final
null pointer. An empty list still supplies a non-null table containing one
null pointer. An empty string is a non-null pointer to a NUL byte. There is
no encoding conversion or `argv[0]` insertion, and environment entries are not
interpreted or inherited automatically.

Only a shared `&list[str]` is accepted. C may read the table and strings during
the call, but must not modify either, retain their addresses, or return an
address into them. A C signature written `char *const argv[]` is compatible
only when its documented behavior obeys these read-only, call-scoped rules.
The binding author makes this promise; Plenty cannot inspect the native body.
No pointer value for the temporary is exposed to Plenty code.

Validation checks all elements for embedded NUL before allocating the packed
buffer. Failure returns the existing `CStrError` without entering C. Adapters
run left to right; failure in a later argument releases earlier buffers.
Native return also releases every buffer. The source list and strings remain
unchanged on all paths. Memory is proportional to the pointer count plus the
terminated byte lengths; layout overflow is `Allocation(CapacityOverflow)`.

This adapter can express `argv`/`envp` inputs; it does not provide C record
layouts, opaque initialization, or the remaining contracts of `posix_spawn`.

## Capture native errno

```text
extern def access_raw(path: &str as c_string, mode: i32, error: &mut i32 as errno) -> Result[i32, CStrError] = "access"
```

`as errno` accepts exactly one `&mut i32` output parameter. It is a Plenty-only
parameter: the example's actual C signature is `int access(const char *, int)`.
Immediately after C returns, before releasing any adapter buffer or making
another runtime call, the adapter snapshots this thread's C `errno` into the
output slot. The declared native return and any `CStrError` wrapper are unchanged.
This also works with void returns and native function-address calls. Ordinary
exclusive borrowing protects the output slot.

The snapshot is unconditional after native entry. It does not clear `errno`,
infer a failure sentinel, retry an interrupted call, or turn the value into a
Plenty error. Interpret it only when the native function's documented return
contract says it is meaningful. Successful calls may leave a stale or nonzero
value. If argument conversion fails, C is not entered and the slot is unchanged.
Do not call another function to retrieve the error after the adapter returns.

The implementation reads GNU/Linux thread-local `errno`, macOS `errno`, or
Windows CRT `errno`. Windows `GetLastError`, library-specific error slots, and
foreign libraries using a separate CRT instance are outside this contract.
Capturing `errno` grants no worker permission: a symbol declaration still needs
[`@thread_safe`](trusted-declarations.md#worker-threads), and function-address
calls remain forbidden on workers. Each worker captures its own thread's slot.
