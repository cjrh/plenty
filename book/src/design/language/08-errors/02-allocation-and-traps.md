# Allocation and traps

Source-level heap construction and allocating operations use checked paths.
`AllocError` has two allocation-free nullary variants:
`AllocError.OutOfMemory` for allocator rejection and
`AllocError.CapacityOverflow` for impossible sizes or layouts. Constructing,
copying, comparing, matching, or propagating it inside Result allocates nothing.
`AllocError` itself is not a family on which `?` operates.

Class instances, tuples, and enum values are inline and construct directly
without allocation. Allocations in their component expressions keep their
own Results. A fallible class initializer can make construction return
`Result[Class, AllocError]`; [class initialization](../03-types/classes.md)
defines cleanup of a partially initialized instance. String literals, scalar
values, and ranges require no heap allocation.

Collections, boxes, and newly allocated dynamic text expose allocation errors.
Collection displays, comprehensions, constructors, growing updates, and
`copy(value)` return Result. Their APIs have no separate `try_` naming family.
Successful reservation permits subsequent insertions within that capacity
without storage allocation. Failed growth preserves logical contents and order,
although capacity may have increased. Failed deep copy preserves the source
and destroys partial output. Exact APIs and input-transfer behavior are in
[collections](../../facilities/01-builtins/collections.md) and
[text](../../facilities/01-builtins/text.md).

Failure does not generally roll back argument effects, consumed generator
steps, external input, or partial writes. Operations specify what is preserved
and what has already been consumed. Allocation-free wrapper transport,
borrowing, inline environments, generator frames, and cleanup bookkeeping do
not make allocations performed by user expressions or destructors disappear.
Cleanup on an allocation-error path must still handle its own fallible work.
No public allocator switching or custom-memory-manager API is exposed.

`value.unwrap()` extracts a success payload without allocation, moving it
when owned. `Err` or `Nothing` deliberately traps. Invalid indexing, absent
dictionary keys in indexed access, division by zero, and invalid range
parameters are also fatal runtime errors; optional lookups provide recoverable
absence where supported. Traps terminate the process without unwinding language
scopes. They are not caught by Result propagation or resource scopes.
