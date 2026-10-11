# Boxed callable environments

`BoxedCallable[[inputs], result]` owns an explicitly allocated callback with a
structural signature. Unlike concrete `Closure`, its type does not include the
producer or capture layout. Different owned environments with the same signature
can share list elements, dictionary values, class fields, enum payloads, and
factory return types. Named `Callable` values can use the same owner type.

`BoxedCallable(callback)` consumes the callback and returns
`Result[BoxedCallable[[inputs], result], AllocError]`. Construction always attempts
one allocation, even for a named function or capture-free closure. There is no
implicit conversion or allocation when assigning or returning a callback. On
allocation failure the consumed environment and its captures are dropped.

The initial contract accepts reusable shared-call callbacks with wholly owned
captures. Borrowed captures, generator-containing environments, mutable closures,
one-shot closures, and callbacks returning references are rejected. Signatures
otherwise use the same rules as `Callable`, including reference parameters and
ordinary owned arguments, with at most 255 parameters plus the hidden environment
slot. A `BoxedCallable` satisfies a matching generic
`Callable` constraint; its affine ownership remains visible to specialization.

Invocation borrows the owner immutably, including through shared or mutable
references, fields, and collection indexes. The binding need not be mutable.
Temporary owners survive argument evaluation and the call, then drop normally.
Live borrowed callbacks prevent moves, replacement, removal, and invalidating
collection changes. Moving a box transfers its sole owner; copying, equality,
membership equality, hashing, and use as a dictionary key or set element are
unsupported, as for concrete closure environments.

Calls, moves, and cleanup allocate nothing beyond allocations explicitly
performed by the callback itself. The heap box retains its concrete environment
descriptor for relocation and destruction, together with a compiler-generated
adapter address. Each capture is dropped exactly once when its owner is replaced
or destroyed. Calls use that adapter and the public signature, so the caller
does not need the concrete capture layout. Formatting shows `<boxed callable>`.

Erasure does not certify captures or indirect effects for concurrency. Boxed
callbacks, including nested fields and container elements, cannot cross worker
or channel boundaries. Retain a concrete environment when worker eligibility
must be checked. Mutable and consuming erased modes require a separate contract.
