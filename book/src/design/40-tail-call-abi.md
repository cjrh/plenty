# Tail-call ABI

Native Plenty functions use Cranelift's `Tail` calling convention. A checked
representation plan is shared by validation and native lowering. An approved
transfer emits `return_call` or `return_call_indirect`; a mismatch at lowering
is an internal compiler error. Unsupported transfers become explicit ordinary
calls before native lowering, preserving argument evaluation and cleanup order.
For direct calls in a recursion cycle, an unsupported source-tail transfer is
a compile error instead; see the
[function contract](05-current-language-contract/07-functions-expressions-and-control-flow.md).

| Arguments and result | Native transfer |
|---|---|
| Scalar or heap-owner arguments, exact-compatible results | Supported |
| Exact-compatible inline result, including ranges, records, nested sums, and concrete generator frames | Supported through the incoming result-area pointer |
| Owned inline argument, including ranges, records, closures, or generator frames | Unsupported; requires storage surviving frame removal |
| Reference, including one to inline storage | Supported when the frontend marks the call with `ForwardsReferences`: every reference it passes originates in a reference parameter of the caller. Without that fact the call retains its frame; see [deterministic destruction](14-deterministic-destruction.md) |
| Indirect `Callable` call | Same result/owned-input ABI checks and the same reference rule |
| Result requiring conversion, wrapping, propagation, or a different result area | Ordinary call; the remaining work happens after it returns |

Callable signatures still exclude generator frames and closure environments;
the ABI plan does not expand the source language's callable types. Concrete
closure invocation uses its existing environment adapter and is not a native
tail-transfer operation.

## Result storage

An inline result's address points into storage supplied by the caller. The
plan checks all result components, their native scalar representations, the
inline byte capacity, alignment, and per-result offsets. Exact source result
types additionally guarantee matching nested relocation metadata.

A tail transfer forwards the incoming hidden result pointer unchanged. It
does not create a result slot in the departing frame. At the end of the chain,
the ordinary return copies/relocates nested inline addresses into that area
before cleaning up locals. Source code cannot access the hidden result area,
so an outgoing source borrow cannot alias it. Public foreign interfaces do not
expose this private ABI.

Different argument counts, register assignments, and stack argument sizes are
allowed; the calling convention handles the native argument transfer. Remaining
owned locals are released before the transfer after arguments have been staged.

## Owned inline arguments

The current ABI passes an address for inline payload bytes. Passing that address
through a native tail call would leave the callee reading the removed frame.
Cranelift 0.131's `StructArgument` support can copy argument bytes into outgoing
stack storage, but does not relocate Plenty's embedded inline addresses. A raw
copy is insufficient for nested records, sums, generators, and closure captures.
The compiler therefore rejects this representation in a native transfer plan;
it does not allocate hidden heap storage to extend its lifetime.

For an ordinary fallback, copied and moved inline arguments already have staged
snapshots. The frame remains alive through the call; locals that are not borrowed
are still dropped before entering the callee. See
[deterministic destruction](14-deterministic-destruction.md).
