# Allocation sites: copies, key views and slices

This inventory covers three families of independent results. It records where
storage is obtained, including paths that only borrow or transfer an existing
value. It is **not** an inventory of all language or std allocation. In
particular it excludes allocations made by expressions that produce the
operands, and allocations internal to user provider callbacks.

**Ordinary** means the runtime policy of the operation, not necessarily system
`malloc`, and not an allocator inherited from an input. **Selected** means the
caller supplies the output allocator; see [retained storage](allocator-storage.md)
for object, backing and release contracts. A retained leaf keeps its existing
provenance in either kind of result. Paths below are relative to the repository
root; native aggregate functions without a path are in
`plenty-runtime/src/aggregates.rs`.

## Copy

| Entry | Destination and ownership |
| --- | --- |
| `copy(value)` | `src/frontend/references.rs`: `copy_or_drop` observes the operand and emits `CollectionOp::TryCopy`. `src/codegen/collections.rs` supplies space for inline data. `collection_at` (33) calls `try_copy` with `CopyDestination::Ordinary`. Ordinary `try_copy_shared` can duplicate a foreign actor-heap string rather than retain it. |
| `out.copy(&value)` | `stdlib/std/alloc.plentyi`: `Allocator.copy` → `__allocator_copy` → `ALLOCATOR_COPY`. [Selected copy lowering](allocator-storage.md#selected-copy) supplies the **element** descriptor, packed value and frame space. `collection_at` calls `try_copy` with `Selected(&allocator)`. This is a local copy, not an actor transfer. Both entries borrow and use `check_explicit_copy`. Selected copy additionally rejects any result type that can carry a loan, including a named class with a slice field. Neither entry materializes views. |

`try_copy` relocates inline bytes into private storage (`ranges::copy_payload`)
before `try_copy_into` walks record fields and active variant payloads. Inactive
payloads cost nothing. `CopyDestination::{collection, boxed, copy_shared}` in
`plenty-runtime/src/aggregates/allocator_owners.rs` separates these cases:

- **New owners.** `try_copy_box` and `try_copy_collection` rebuild boxes, lists,
  dictionaries and sets. With `Selected`, object, bookkeeping, rows, order
  links and buckets, where present, use the output provider. Finite sets copy
  their bits into the owner; they need no bucket or row block. A dictionary
  with matching sequential layout can copy buckets instead of rehashing.
  Nested copies use the same destination. No heap staging area is needed for
  inline records: private result rows or box content hold them during the walk.
- **Existing leaves.** Selected copies retain immutable heap text and nested
  `Shared` handles, without allocating new backing or traversing the shared
  content. A bare `Shared` requires `share()`. Inline scalars, ranges and plain
  array payloads need no provider call.
- **Failure.** `abandon_copy` releases only completed fields and clears slots
  that still alias the source. The `OwnedValue` guard drops initialized rows,
  including a copied key if its value fails. Refusal by a selected provider
  returns its error without fallback. The source remains usable.

Evidence distinguishes three boundaries:

1. `tests/test_fallible_copy.rs`: `failed_dictionary_value_copy_releases_the_pending_key_and_prior_entries`
   and `immutable_values_and_inactive_owned_variants_copy_without_allocating`
   exercise the ordinary path.
2. `plenty-runtime/src/aggregates/allocator_owners/tests/recursive_copy.rs`:
   `recursive_copy_routes_allocations_retains_leaves_and_unwinds_each_failure`
   calls the selected walker **directly**. With the source provider closed to
   new requests, it rejects each of 21 result allocations, checks releases,
   lookups and shared reference counts, then checks independence on success.
   `selected_copy_entry_relocates_inline_variant_before_copying_its_box`
   separately calls the native opcode.
3. `tests/test_std_alloc_copy.rs`: `selected_copy_reads_typed_references_and_returns_inline_results`
   uses the public wrapper with a zero-budget provider;
   `inline_copy_requires_no_heap_scratch` also forbids ordinary allocation.
   `selected_copy_on_worker_unwinds_nested_failure_and_retains_only_shared_content`
   checks the provider's `CapacityOverflow`, partial releases, a later successful
   copy, and the shared atomic after source destruction. These are compiled calls,
   not direct walker tests.

## Dictionary key views and independent results

| Surface | Lowering and allocation site | Failure / source |
| --- | --- | --- |
| `d.keys()` and observations | `CollectionOp::KeyView` in `src/codegen/collections.rs` forwards the dictionary reference. Iteration, length, membership, equality (`src/frontend/operators.rs`) and set relationships require no snapshot. | Loan, no result allocation. `values()` and `items()` also project existing storage. Mutating filters are not independent results. |
| `keys.to_set()` | `KeyViewToSet` → `KEY_VIEW_TO_SET` → `try_members_copy(..., Ordinary)`. Allocates a set of keys, not a dictionary, and does not duplicate values. Matching sequential layouts can reuse the hash information by copying buckets. | Reserves for the source length before copying. On failure drops initialized output, not the dictionary. |
| `out.to_set(keys)` | `Allocator.to_set` → `__allocator_key_view_to_set` → `ALLOCATOR_KEY_VIEW_TO_SET` → `try_key_view_to_set_in` → `try_members_copy(..., Selected)`. Same member walk as `keys.to_set()`, but the result owner and hash backing (or just the finite-set owner) use `out`. The result descriptor is `set[K]`, not `dict[K, V]`. | Copies only keys from the borrowed dictionary. Provider refusal releases partial storage and returns its `AllocError`, without changing keys or values. |
| Ordinary union, intersection, difference, symmetric difference involving a key view | `src/frontend/collections.rs` lowers set-like method operands; `src/codegen/collections.rs` passes `KeySetAlgebra` a **set-of-keys** result descriptor. `KEY_VIEW_*` → `try_set_algebra` → `try_set_algebra_with(..., Ordinary)`. | View/view or matching set/view (either order). Counts result members and reserves before copying. On error both inputs remain unchanged. |
| `out.union(left, right)` and its three algebra siblings | Trusted direct calls specialize the wrapper for shared sets and/or key views. `ALLOCATOR_SET_*` → `try_set_algebra_in` → `try_set_algebra_with(..., Selected)`. The compiler supplies the **result** `set[K]` descriptor independently of both input descriptors. | Borrows and probes keys in place; no input conversion or dictionary value copies. The selected provider pays for the independent result and any new key storage. Refusal unwinds partial output and preserves both sources, without fallback. |
| `d.keys_snapshot()` / `values_snapshot()`; `d.into_keys()` / `into_values()` | Separate dictionary **list** results: `TryKeys` / `TryValues` → `try_dictionary_snapshot`. Reserves ordinary list storage for all entries before retaining or transferring in insertion order. | Borrowed snapshots retain non-affine elements. An affine projected element must be transferred from a unique temporary or consumed dictionary, not duplicated from a borrow. `into_*` consumes the dictionary even if reserve fails; it does not return it. |

Independent key sets require duplicable keys; there is no implicit `set(view)`
or `list(view)` conversion. `try_push_members` retains simple keys, duplicates eligible affine keys in the
final row, or inserts bits for a finite set. Where layouts permit, the **result
table** temporarily indexes source slots and unused result value slots hold
known hashes; there is no separate scratch collection. The temporary index is
cleared before building the final index or rolling back.

Evidence: `tests/test_dictionary_views.rs` (`observations_never_allocate_and_algebra_failure_preserves_source`,
`tuple_key_views_compare_and_materialise_keys_with_independent_value_types`,
`each_algebra_allocation_can_fail_without_changing_either_source`,
`into_keys_allocation_failure_consumes_and_drops_affine_values_once`);
`tests/test_dictionary_snapshots.rs` (`each_snapshot_allocation_can_fail_and_retry`).

For selected algebra, `plenty-runtime/src/aggregates/allocator_owners/tests/set_algebra.rs`
checks result descriptors, provider routing and rollback, and that mixed calls
read keys without reading dictionary values. `tests/test_std_alloc_set_algebra.rs`
covers the public calls, including operand order, matching key types, local
loans, refusal and result lifetime.

For selected conversion, `plenty-runtime/src/aggregates/allocator_owners/tests/key_view_to_set.rs`
refuses each hash-result allocation, checks source lookups and value cleanup,
and exercises the native opcode with a finite-set descriptor.
`tests/test_std_alloc_key_view.rs` exercises the **public method** with a named
view and a direct `keys()` projection, including a scoped worker, provider
failure and growth after the source and allocator handle are dropped.

## Owning slices and their non-owning lookalikes

`src/frontend/slices.rs` dispatches on the receiver, not just the syntax.
`out.slice(&xs, ...)` and runtime bracket paths compute `(first, step, count)`
with `slice_positions`, not a heap list of positions. `out.copy_view(v)`
instead reads a contiguous range whose bounds are already part of the view.

| Surface | Destination, nested data and failure |
| --- | --- |
| List brackets `xs[a:b:k]` | `ListSlice` → `try_list_slice_stepped`: ordinary list reserved for the selected count. Only adjacent rows whose elements own nothing and have no inline payload copy in bulk. Typed insertion gathers other rows and gives inline payloads destination-local addresses. Managed elements use `try_copy_into(..., Ordinary)` in their final rows. On failure the current slot is cleared and the guard drops the initialized prefix. Source is only read. |
| `out.slice(&xs, start, stop, step)` | `__allocator_list_slice` → `ALLOCATOR_LIST_SLICE` → `try_list_slice_stepped(..., Selected)`. Reserves for the selected count; even an empty list gets selected bookkeeping and object storage. Managed rows use the same destination recursively for new boxes and collections; text and `Shared` leaves can retain their backing. Provider refusal unwinds partial rows and storage without changing `xs` or trying another provider. |
| List method `xs.slice(a, b)` | Opcode 45 → `try_list_slice`: clamp and reserve an ordinary result list before any transfer. Retain non-affine elements; for affine elements the receiver must be a unique temporary. Move the selected elements **with their existing backing** and let temporary cleanup drop the rest (all on failed reserve). Not bracket deep-copy semantics. |
| `out.copy_view(v)` | `Allocator.copy_view` in `stdlib/std/alloc.plentyi` constructs a selected `list_with_capacity[T](len(v))`, then appends `out.copy(element)?` for each borrowed element. The view iterator reads both packed-byte and list-backed `&[u8]`; neither requires a temporary input list. New nested owners use the selected provider. A refusal drops the initialized prefix and list storage without modifying the source or retrying elsewhere. |
| Ordinary list-view materialization | `view_slice` constructs an adjacent subview; `view_copy` / `view_share` in `src/frontend/view_methods.plenty` create an ordinary empty list, reserve and append `copy(element)?` or `element.share()`. Partial lists drop on error, the view is only read. No explicit step here; for a byte view choose `list(&v[a:b])?` or extend a `Bytes`. |
| `str.slice(a, b)`, text brackets and text-view copies | In `plenty-runtime/src/strings.rs`, the method uses `try_slice` / `scalar_range`; brackets use `try_stepped_slice`. For adjacent scalars copy the UTF-8 interval; for strided ASCII gather bytes, otherwise count encoded bytes and fill the final output in forward or reverse order. Short results can be inline; no gathered-character buffer. A view's `[:]` uses `TEXT_VIEW_COPY` in `aggregates/text_views.rs`; bounded/stepped copies use `try_stepped_slice_range` via `aggregates/text_ranges.rs` (both under `plenty-runtime/src/`). Only the owned text can require allocation; failure preserves the input. |
| `bytes` literal / `Bytes` brackets | `BytesOp::SliceLiteral` / `Slice` → `BYTES_SLICE` in `src/bytes.rs`. `plenty-runtime/src/aggregates/byte_storage.rs`: `create(count)` reserves ordinary vector storage before allocating the header; if the header fails the vector drops. Copy or gather directly into that buffer and wrap it as `Bytes`. Source is only read. |
| `Bytes.slice(start, stop)` | `stdlib/std/bytes.plenty`: check `0 <= start <= stop <= len` (trap, not a clamped slice), `__bytes_new(width)?`, then `__bytes_append` from a borrowed byte range. Allocation errors do not change the source. |
| Named `clist` brackets | `ClistOp::Slice` → `slice` / `reserve` in `plenty-runtime/src/aggregates/clist.rs`. Reserve one `Columns` allocation for a nonempty result; copy one contiguous interval or gather strided primitive rows **per column**. Empty result can be null. Failed reserve leaves the source unchanged. |
| Adjacent borrowed ranges | `&xs[a:b]`, allowed exclusive ranges and subviews lend existing storage; `&s[a:b]` lends UTF-8 by scalar bounds. No new owner, nested duplication or allocation-error result for the loan. |
| Tuple brackets | `tuple_slice` computes literal positions in the frontend and projects non-affine components to an inline tuple (or unit). No runtime position buffer, allocator or `Result`. |

Evidence by path:

- `tests/test_slice_syntax.rs`: `list_slices_copy_owned_elements_and_share_handles`,
  `every_slice_allocation_failure_returns_err_and_keeps_the_source`,
  `clist_slice_allocation_failure_returns_err_and_keeps_the_source`,
  `tuple_slices_select_components_at_compile_time`.
- `tests/test_slicing.rs`: `temporary_slice_transfers_selected_owners_and_cleans_every_failure`
  and `string_slices_allocate_only_the_final_string_and_recover_on_failure`.
- `tests/test_slice_views.rs`: `views_add_no_allocations` versus
  `view_copies_recover_from_failure_mid_copy`;
  `tests/test_str_views.rs`: `as_bytes_allocates_nothing_and_direct_view_copy_reports_failure`.
- `tests/test_std_alloc_copy_view.rs`: public method on empty and nonempty ranges,
  packed bytes and list-backed bytes; partial nested-copy refusal on a worker,
  retained growth after source and handle drop, and compile-time rejections.
- `tests/test_std_bytes.rs`: `ranges_follow_one_rule_without_clamping_or_negative_offsets`
  and `second_allocation_failure_releases_the_first`.

For **selected** list slices, `plenty-runtime/src/aggregates/allocator_owners/tests/list_slice.rs`
checks growth through the retained provider, each refusal in a nested copy,
shared leaves and source independence. It also checks that ordinary brackets do
not inherit a selected source provider. `tests/test_std_alloc_slice.rs` covers
the public method: default and explicit bounds, step refinement, worker loans,
provider refusal and result lifetime.
