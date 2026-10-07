# Allocation policy

Runtime object allocations carry a private allocation prefix identifying their
allocator. The generated-code-visible Header and payload offsets are unchanged;
freeing uses the stored allocator, never a mutable global choice. The initial
internal allocator interface requires process-lifetime callback tables. Ordinary
objects currently select the global allocator, and Vec-backed container buffers
still use it directly. This is provenance groundwork, not a public custom-memory
manager API or support for scoped allocator lifetimes.

`str.repr(value)` borrows any printable value and returns
`Result[str, AllocError]`, including escaped strings and structural aggregates.
`print(value)` borrows the value and returns `Result[(), IoError]`. It uses
raw top-level strings and a trailing newline. String printing needs no temporary
allocation; other formatting uses checked buffer growth. Formatting allocation
failure emits no bytes; an I/O error can leave partial output. Neither invokes
user-defined formatting methods. There is no separate aborting print API.

New practical I/O APIs must return explicit errors, including allocation failure
in their own buffers and result construction. They must not hide infallible
`String` growth behind a fallible public signature. Input consumption and partial
external writes cannot generally be rolled back; their contracts must say so.
All source-level heap construction and allocating operations use checked paths.
Literals and comprehensions return `Result` directly; public API names have no
`try_` prefix. `?` and `match` handle errors, while `.unwrap()` explicitly traps
on `Err` or `Nothing`. Unwrapping moves owned payloads and allocates nothing.
The trap terminates without unwinding; it is a deliberate caller choice.
Allocator provenance stays with owners; no public allocator switching API exists.
