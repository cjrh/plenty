# Tail-call ABI

Native Plenty functions use Cranelift's `Tail` convention. Validation and lowering
share a checked representation plan. Approved transfers emit `return_call` or
`return_call_indirect`; disagreement at lowering is an internal compiler error.
Unsupported transfers become ordinary calls before lowering. Direct source-tail
calls in a recursion cycle instead fail checking when no safe plan exists; see
the [function contract](../../language/06-functions/index.md).

| Representation | Native transfer |
|---|---|
| Scalar/heap-owner arguments and exact-compatible result | Supported |
| Exact-compatible inline result, including ranges, records, sums, concrete frames | Supported through incoming result storage |
| Owned inline argument, including range, record, closure, or frame | Unsupported: its bytes must outlive frame removal |
| Reference argument | Supported only when `ForwardsReferences` proves every passed reference originates in a reference parameter |
| Indirect `Callable` call | Same result, owned-input, and reference checks |
| Conversion, wrapping, propagation, or incompatible result area | Ordinary call; remaining work follows return |

Concrete closure invocation uses its environment adapter and does not perform a
native tail transfer. ABI capabilities do not expand allowed callable signatures.

## Storage and cleanup

An inline result uses caller-supplied storage. The plan checks native scalar
representations, payload capacity, alignment, and component offsets; exact source
types ensure compatible nested relocation metadata. A transfer forwards the
incoming hidden pointer unchanged. Final return relocates the result there before
local cleanup. Source code cannot borrow this hidden result area.

Arguments are staged before caller-owned locals are released. Different argument
counts, registers, and stack argument sizes are handled by the calling convention.
Owned inline arguments currently pass addresses into caller storage. Cranelift's
`StructArgument` can copy bytes but cannot relocate Plenty's nested addresses, so
a raw copy cannot safely remove the frame. No hidden heap allocation extends it.
Ordinary fallback keeps the frame alive and uses staged snapshots; unborrowed
locals still receive the source-defined pre-call cleanup.
