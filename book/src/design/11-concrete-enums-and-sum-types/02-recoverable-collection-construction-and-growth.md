# Recoverable collection construction and growth

`AllocError` is a builtin nominal enum with two nullary variants:
`AllocError.OutOfMemory` and `AllocError.CapacityOverflow`. It shares the binary
inline representation of standard sums and never owns memory. Constructing,
copying, comparing, matching, or propagating it inside a `Result` does not
allocate. `AllocError` itself is not an operand or return family for `?`.
Unsupported allocator layouts remain a future concern when custom allocators exist.

Explicit collection types expose `new()` and `with_capacity(capacity: i64)`:
`list[T].new()`, `set[T].with_capacity(n)`, and
`dict[K, V].with_capacity(n)` return `Result` with the collection as its
success payload and `AllocError` as its error payload. Collection type aliases,
including imported aliases, expose the same constructors. This is concrete
builtin type-method syntax, not general generic functions or static class methods.

Both constructors produce an empty collection. `new()` allocates only its
owner header; `with_capacity(n)` also reserves room for at least `n` entries,
including hash storage where needed. The capacity expression is evaluated once.
Negative counts and impossible layouts return `CapacityOverflow` before attempting
any allocation. Allocator rejection at any subsequent stage returns `OutOfMemory`.
Construction cleans up any buffers already reserved before returning an error.
No partially initialized collection or user destructor is exposed.

The runtime validates and reserves buffers in a local Rust value before allocating
and initializing its owner header. Rust cleanup handles partial reservation; after
publication, the intrusive destruction queue releases buffers and frees the header
with its matching layout. Ordinary constructor syntax uses this same checked path.

The following exclusive methods return `Result[(), AllocError]`:

| Receiver | Method | Contract |
| --- | --- | --- |
| `list[T]`, `set[T]`, `dict[K, V]` | `reserve(additional: i64)` | Reserve room for at least this many more entries beyond the current length |
| `list[T]` | `append(value: T)` | Append one element |
| `set[T]` | `add(value: T)` | Insert an element if absent |
| `dict[K, V]` | `insert(key: K, value: V)` | Insert or replace a value, preserving key insertion order |

They work on mutable bindings, exclusive references, and mutable class fields.
Callers handle the returned `Result` with `match` or propagate it with `?`.
Negative reservation counts and capacities exceeding the addressable buffer
layout return `CapacityOverflow`; allocator rejection returns `OutOfMemory`.
The return type remains fallible even when a particular call needs no growth.

Growth validates sizes and reserves both entries and hash storage before
committing a mutation. Failed reservation/insertion preserves logical contents,
order, and lookup behavior; callers must not depend on capacity being unchanged.
Hash rebuilding uses preallocated storage and never invokes user code. A
successful reserve permits that many subsequent insertions without storage
allocation. Existing-key replacement and duplicate set insertion do not grow
storage. Argument evaluation and user cleanup can still allocate independently.

Insertion consumes its arguments on success and failure. A failed insertion
destroys an owned input instead of returning it; a caller needing to keep the
input can reserve before moving it. The native helper borrows inputs and retains
only successfully stored values; the compiler releases its input owners on both
paths. The returned error needs neither formatting nor a heap-backed enum record.

`copy(value)` returns `Result[T, AllocError]`, where `T` is the observed
operand type. An owned
binding is observed rather than consumed, reference operands copy the referred-to
value, mutable contents are recursively duplicated, and immutable storage such as
strings can be shared. Scalars and wholly immutable values require no allocation.
Classes with custom cleanup, aggregates containing them, and generators remain
uncopyable. The argument expression runs once; its own construction can still fail
under the failure policy of that expression.

A failed deep copy leaves the source unchanged and releases all newly owned
values. Collection copies reserve the final entry count before copying payloads;
temporary ownership guards cover pending keys, pending values, and partial
collections. Record copies track their initialized field prefix and release only
that prefix on error, then free the header directly. They never run a whole-record
destructor on a partial object. Active inline sum payloads follow the same rules;
inactive owned variants require no allocation. Propagating a
failed `copy` uses the ordinary `?` cleanup rules without allocating an error.

Dictionary indexed assignment replaces an existing entry without allocation;
an absent key traps. Use `insert(key, value)` to add keys and handle its Result.
List indexed replacement, clear/reverse/removal, and structural comparisons need
no allocation. Custom destructors must avoid allocating or handle their own
operation results on an OOM recovery path.

Collection buffers remain owned by Rust `Vec` with the same fixed global allocator
for growth and deallocation. Runtime allocator switching is not exposed. Per-value
allocator handles, context lifetime, and matching deallocation must be implemented
before adding custom/global allocator selection; merely changing an allocation
function pointer would lose allocator provenance.

The instrumented runtime can reject every allocation after a chosen number of
successful allocator calls, including reallocations. Native tests cover every
allocation site in construction/growth/reservation and nested copying, retry, preserved contents, moved-input
cleanup, and allocation-free handling while allocation remains disabled.
