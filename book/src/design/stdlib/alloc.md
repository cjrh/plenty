# `std.alloc` — explicit retained allocators and safe byte blocks

`std.alloc` exposes an explicit system allocator and compiled custom providers.
It constructs ordinary `Box[T]`, `list[T]`, `dict[K, V]`, `set[T]`, `clist[T]` and packed [`Bytes`](bytes.md) owners.
Each retains the selected allocator for its own storage, so growth and cleanup
do not require the caller to keep the allocator handle. This is not comprehensive allocation control; see the
[remaining B13 scope](../../backlog.md#b13-explicit-allocator-implementation-sequence).

This module also provides validated memory-layout requests and affine
zero-initialized byte blocks with checked access and disjoint splitting. These
are safe allocator backing types for custom policies: no raw pointers escape,
no copying is permitted, and blocks retain their provider through all operations.
Fresh block construction allocates metadata even for an empty range. Valid
layouts do not guarantee that metadata and payload can both be allocated.

Native runtime support is necessary for builtin storage and thread-safe retained
handles. The public wrappers are written in Plenty. The separate
[`std.experimental.alloc`](experimental-alloc.md) module remains thread-confined;
its handles and owner types are not interchangeable with these.

## Examples

### Explicit system allocator for lists

```plenty
import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    mut values = owner.empty_list[i64]()?
    values.append(7)?
    values.append(8)?
    drop(owner)
    print(values)?
    Ok(())
```
```output
[7, 8]
```

### Safe byte blocks

```plenty
import std.alloc as alloc

def main() -> Result[(), alloc.LayoutError | AllocError | IoError]:
    owner = alloc.system()
    request = alloc.layout(10, alignment=64)?
    mut left = owner.block(&request)?
    print(left.len(), left.get(0))?
    mut right = left.split_off(5)
    drop(owner)
    left.set(4, 33u8)
    right.set(0, 44u8)
    print(left.len(), right.len(), left.get(4), right.get(0))?
    Ok(())
```
```output
10 0
5 5 33 44
```

## API

### Layout validation (allocation-free)

| Signature | Behavior |
| --- | --- |
| `layout(size: i64, alignment: i64 = 8) -> Result[Layout, LayoutError]` | Returns a validated, copyable request without allocating. Zero size is valid. Private fields prevent direct construction or mutation. |
| `Layout.size(self) -> i64` | Returns the requested size passed to `layout()`, not any rounded extent. |
| `Layout.alignment(self) -> i64` | Returns the requested alignment. |

`LayoutError` is copyable. Validation first rejects nonpositive or non-power-of-two
alignment with `InvalidAlignment`. It then rejects sizes outside
`0..=i64.MAX - (alignment - 1)` with `Allocation(AllocError.CapacityOverflow)`.
The factory never returns `OutOfMemory`. The error name distinguishes request
validation from the builtin `AllocError`; scalar alignment permits named errors
for invalid configuration rather than requiring a prevalidated integer domain.

### Allocator handles and lists

| Signature | Ownership, allocation and errors |
| --- | --- |
| `system() -> Allocator` | Creates an explicit system-root handle without allocation. It has no per-instance state or reference count to allocate. |
| `Allocator.share(self) -> Allocator` | Borrows the handle and returns another owning handle without allocation. Custom handles atomically retain their state; the system root has no reference count to increment. |
| `Allocator.empty_list[T](self) -> Result[list[T], AllocError]` | Borrows the allocator and creates an empty builtin list with zero row capacity. Object and bookkeeping storage still allocate. |
| `Allocator.list_with_capacity[T](self, capacity: i64) -> Result[list[T], AllocError]` | Borrows the allocator and creates an empty builtin list with at least the requested element capacity. Zero is valid; negative or unrepresentable capacity returns `CapacityOverflow`. |

### Copy a list slice with an output allocator

`Allocator.slice[T](self, source: &list[T], start: i64 | Unset = Unset.Unset, stop: i64 | Unset = Unset.Unset, step: NonZero[i64] = 1) -> Result[list[T], AllocError]`
borrows the allocator and the source. `out.slice(&xs, a, b, k)?` copies the
same elements, in the same order, as `xs[a:b:k]?`, but selects `out` for the
new list. Neither form inherits the source list's provider.

`Unset` is a copyable enum with one variant, `Unset.Unset`. Passing it for
`start` or `stop` is the same as omitting that argument. Integer bounds count
from the end when negative and clamp to the list, with an exclusive stop.
With a positive step the default range runs from zero to the length. With a
negative step it runs from the last element to *before* the first; `stop=-1`
instead names the last element. A nonzero step literal converts implicitly;
`0` is a compile error. For a computed `i64`, use `NonZero.try_from(k)?`:
zero returns `RangeError.OutOfRange` at the conversion, not at `slice`.

Only selected elements are duplicated. As in a [list bracket slice](../language/04-expressions/slices.md#receivers-and-results),
`T` need not be `CopyType`, but its owned parts must support explicit copying:
recursive owned data and custom destructors are rejected even for an empty
range. Elements cannot carry loans. Newly copied boxes and collections use
`out` throughout; immutable text can keep its backing, and each `Shared` handle
(including a whole list element) shares the same content.

The result has at most `len(source)` elements. Even an empty result allocates
its list object and bookkeeping. New owners retain the output provider for
growth and cleanup after the source and handle are dropped. An unrepresentable
allocation size returns `CapacityOverflow`; a provider refusal returns that
provider's `AllocError` without fallback. Either failure releases partial
copies and leaves the source intact.

This method takes a list, not a slice view, text, bytes or a columnar list.
Use `&xs[a:b]` to borrow a contiguous range without allocating. The two-bound
[list method](../facilities/01-builtins/lists.md#slices) `temporary.slice(a, b)` can
transfer affine elements with their existing storage instead of copying them.

### Copy a columnar list slice

`Allocator.slice_clist[T](self, source: &clist[T], start: i64 | Unset = Unset.Unset, stop: i64 | Unset = Unset.Unset, step: NonZero[i64] = 1) -> Result[clist[T], AllocError]`

`out.slice_clist(&rows, step=-1)?` makes an independent columnar list with
its rows in reverse order. Bounds and steps follow the [list slice rules](#copy-a-list-slice-with-an-output-allocator).
Like `rows[a:b:k]?`, the call requires access to every field of the
[primitive row class](../facilities/01-builtins/clists.md); `@copy` is not required.
It copies the selected values column by column without running initializers or
materializing temporary row objects. The result carries no loan from either input.

A single selected block holds the owner and the columns for at most
`len(source)` copied rows, with capacity rounded up for alignment. An empty
result still requests an owner so future growth uses `out`. An unrepresentable
layout returns `CapacityOverflow`; provider refusal returns its `AllocError`.
Neither changes the source or retries elsewhere. After the handle is dropped,
`reserve` and growing `append` still use its provider, as does final release.
A bracket slice of the result does **not** inherit its provider.

[Checked serialization](serial.md) accounts for callbacks even when a clist's
provider is not visible at the call site. When the program emits this method
and registers a compiled provider, checked segments that may grow or release
a clist have no finite stack budget. Reading the count or a scalar field
does not itself invoke a callback.

For [task joins and pool waits](../facilities/03-concurrency/channels.md#which-workers-can-wait),
constructing a selected clist can wait. When both the slice method and a
compiled provider are present, growing or dropping a clist can wait too,
including cleanup of a field, local, or queued message. The owner type does
not distinguish ordinary from selected storage. If a worker can construct the
slice through an allocator it receives, or grow or release a clist reachable
from its inputs, the [join-under-lock check](../facilities/03-concurrency/mutex.md#waits-the-compiler-rejects)
includes the mutex types held by compiled provider state. A shared borrow used
only to inspect a count or a scalar field does not call the provider.

### Share a view of handles into a list

`out.share_view(source)` takes `&[Shared[T]]` and returns `Result[list[Shared[T]], AllocError]`. The result list uses `out` for storage, growth and release. Each element shares its existing payload, including repeated handles to the same object. Neither the payloads nor the source list move. Allocation failure leaves the source unchanged and never retries on the system. Unlike `copy_view`, this operation explicitly asks to share each handle.

### Copy a slice view into a list

`Allocator.copy_view[T](self, source: &[T]) -> Result[list[T], AllocError]`
borrows the allocator and reads the view in order. Pass `out.copy_view(v)?`
when a helper has only the view, or `out.copy_view(&xs[a:b])?` to copy an
adjacent range. The result contains exactly `len(source)` elements. A `&[u8]`
over packed bytes (such as a `Bytes` buffer) and one over a `list[u8]` both
produce `list[u8]`, not `Bytes`. Views have no step; for a strided list copy
use [`out.slice`](#copy-a-list-slice-with-an-output-allocator) on the owner.

Each element follows [`out.copy`](#copy-with-an-output-allocator): newly
copied boxes and collections use `out` recursively, while immutable text and
nested `Shared` handles can keep their backing. Elements must be storable,
loan-free and eligible for explicit copy; they need not be `CopyType`.
Unlike ordinary [`list(v)?` and `v[a:b]?`](../language/04-expressions/slices.md#operations-on-views),
which call `share()` for a bare `Shared` element, `copy_view` rejects that
element type. There is no `share_view` method.

The result owns its list storage, even when empty, and retains the output
provider for growth and cleanup. It carries no source loan: a named view still
borrows its source until its last use, but the list can outlive both the source
and the allocator handle. An unrepresentable capacity returns `CapacityOverflow`;
a provider refusal returns the provider's `AllocError` without fallback. On
failure the method drops any partial list and copied elements. The source is
unchanged on either return.

### Copy a byte slice into packed storage

`Allocator.slice_bytes(self, source: &[u8], start: i64 | Unset = Unset.Unset, stop: i64 | Unset = Unset.Unset, step: NonZero[i64] = 1) -> Result[Bytes, AllocError]`
reads a byte view and returns an independent `std.bytes.Bytes`, not the
`list[u8]` that [`copy_view`](#copy-a-slice-view-into-a-list) returns.
Pass a `bytes` literal, a borrowed `Bytes` or `list[u8]`, or a subview. Bounds
apply to **the view**, not the underlying owner. For example,
`out.slice_bytes(&buf[2:6], step=-1)?` reverses just those four bytes if
`buf` is at least six bytes long.

As with [list slices](#copy-a-list-slice-with-an-output-allocator), negative
bounds count from the end, bounds clamp, and the stop is exclusive. Omit a
bound or pass `alloc.Unset.Unset` (with `import std.alloc as alloc`). For a
reverse step, an omitted stop lies *before* index zero; `stop=-1` names the
last byte instead. A nonzero literal step is accepted directly; convert a computed
`i64` with `NonZero.try_from(k)?` before the call.

The owner and its packed payload use `out`; an empty result still allocates
its owner. Only the selected bytes are copied (at most `len(source)`), directly
into their final storage without a temporary list.
The result carries no source loan and keeps the output provider for growth and
cleanup after both source and allocator handle are dropped. Unrepresentable
storage returns `CapacityOverflow`; a refusal returns the provider's `AllocError`
without fallback. Partial storage is released and the source is unchanged.

A [checked serializer](serial.md) cannot bound the stack used by a compiled
provider behind a `Bytes`. If the program emits both `slice_bytes` and compiled
provider registration, a segment that may grow or release byte storage can
return `Fault.StackLimit` before the segment runs, even for an ordinary buffer
or one with spare capacity. If the adapter's rejection path cannot be bounded
either, the compiler rejects the program instead.

By contrast, `buf[a:b:k]?` uses the same bounds but ordinary output storage,
regardless of `buf`'s provider. `buf.slice(a, b)?` also uses ordinary storage,
but takes no step and traps unless `0 <= a <= b <= buf.len()`.

### Join a generator of strings

`out.join_text_stream(separator, source)` borrows a `&strview` and consumes a `Generator[str]`. It returns `Result[str, AllocError]`. The join copies each piece into a growable UTF-8 byte buffer, inserting the separator only between pieces. It releases its handle to the piece before resuming the generator. The generator's own allocations keep their original policy.

Both the scratch buffer and any non-inline result use `out`. Even an empty stream can fail when creating scratch. During finalization, scratch and the result can both be live. Refusal drops the scratch, current item and generator (including its captures); it neither retries nor returns a resumable generator. Use `join_text` for an existing view to avoid scratch entirely.

### Join a borrowed view of strings

`out.join_text(separator, parts)` borrows a `&strview` and a `&[str]`, returning `Result[str, AllocError]`. The separator goes **between** elements, including empty elements. An empty view yields empty text. Even a single long element is copied into `out`, not retained from the input. The method measures the borrowed pieces, then writes directly into inline text (up to seven UTF-8 bytes) or one selected block. It uses no scratch buffer or system fallback. Failure leaves both inputs unchanged. This method does not accept one-pass generators; ordinary `separator.join(parts)` is unchanged.

### Remove a text prefix or suffix

`out.removeprefix_text(source, prefix)` and `out.removesuffix_text(source, suffix)` borrow whole strings or substrings (`&strview`) and return `Result[str, AllocError]`. They remove at most one exact match at the named end. An empty or absent affix leaves the contents unchanged, but the result is still independent: up to seven UTF-8 bytes fit inline, otherwise one block comes from `out`. No scratch or fallback is used. Ordinary `.removeprefix()` and `.removesuffix()` are unchanged.

### Split text on an explicit separator

`out.split_text(source, separator)` borrows two whole strings or substrings (`&strview`) and returns `Result[list[str], AllocError]`. Like ordinary `.split(separator)`, it matches literally from left to right and preserves empty fields. Empty input gives one empty field. A literal empty separator is rejected at compile time; a computed empty separator traps before allocation rather than returning `AllocError`.

The list uses `out`, as does each field longer than seven UTF-8 bytes. Even when nothing matches, a long field is copied rather than shared with the source. On allocation failure the list and all completed fields are released, both inputs remain usable, and there is no system retry. Whitespace and line splitting are not part of this method.

### Split text into lines

`out.splitlines_text(source, keepends=False)` borrows a whole string or substring (`&strview`) and returns `Result[list[str], AllocError]`. It uses the same [line boundaries as ordinary `.splitlines()`](../facilities/01-builtins/text-splitting.md#lines): CRLF counts once, blank lines keep their positions, and a final terminator does not add another line. Pass `keepends=True` to include the original terminator in each line.

The list uses `out` even for empty input, so later growth of that empty list also uses `out`. Each line is copied independently: up to seven UTF-8 bytes (including any retained ending) fit inline; longer lines use `out`. There is no intermediate list or system retry. Refusal releases partial output and leaves the source usable.

### Convert text case with an output allocator

`out.lower_text(source)` and `out.upper_text(source)` borrow a whole string or substring (`&strview`) and return `Result[str, AllocError]`. They use the same per-scalar Unicode mappings as `.lower()` and `.upper()`: `ß` uppercases to `SS`, and `İ` lowercases to `i` plus a combining dot. The **mapped** UTF-8 length determines storage. Up to seven bytes fit inline; longer results use one block from `out`, even when no character changes. Neither method needs scratch storage or retries on the system; refusal leaves the source usable. Ordinary case conversion retains its system-storage policy.

### Trim text with an output allocator

`out.strip_text(source)` removes Unicode `White_Space` from both ends; `out.lstrip_text(source)` and `out.rstrip_text(source)` remove it only from the left or right. All three borrow a whole string or substring (`&strview`) and return `Result[str, AllocError]`. Interior whitespace is preserved. Empty or all-whitespace input yields empty text. Up to seven retained UTF-8 bytes fit inline; longer results use one block from `out` even if nothing was trimmed. There is no scratch or fallback, and refusal leaves the source usable. Ordinary `.strip()`, `.lstrip()`, and `.rstrip()` keep their system-storage policy.

### Replace literal text with an output allocator

`out.replace_text(source, old, new)` borrows three whole strings or substrings (`&strview`) and returns `Result[str, AllocError]`. Matches are literal, non-overlapping and replaced from left to right. An empty `old` inserts `new` at each Unicode scalar boundary, including both ends. The method checks the final size before allocation. Up to seven UTF-8 bytes fit inline; longer results use one block from `out`, even if nothing matched. It needs no scratch or system fallback. Refusal leaves all inputs usable. Ordinary `.replace()` keeps its system-storage policy.

### Repeat text with an output allocator

`out.repeat_text(source, count)` borrows a whole string or substring (`&strview`) and returns `Result[str, AllocError]`. A nonpositive `i64` count or an empty source yields empty text. If the repeated length exceeds `i64.MAX`, it returns `AllocError.CapacityOverflow` before calling the provider. Up to seven UTF-8 bytes fit inline. Longer results use one block from `out`, even when `count` is one. No scratch storage or fallback is needed; refusal leaves the source usable. Ordinary `source.repeat(count)` keeps its system-storage policy.

### Concatenate text with an output allocator

`out.concat_text(left, right)` borrows two text views (`&strview`), including whole strings, and returns `Result[str, AllocError]`. Up to seven **UTF-8 bytes** fit inline. For longer results it writes both ranges directly into one block from `out`, even if one input is empty. The result can outlive both inputs and the allocator handle. Refusal leaves both inputs usable, without a system retry. Ordinary `+` and `.concat()` keep their system-storage policy.

### Copy a text slice with an output allocator

`Allocator.slice_text(self, source: &strview, start: i64 | Unset = Unset.Unset, stop: i64 | Unset = Unset.Unset, step: NonZero[i64] = 1) -> Result[str, AllocError]`
returns independent immutable text. Pass a whole string, a literal, or a
[substring view](../language/04-expressions/slices.md#substring-views).
Bounds count **Unicode scalars within the input view**, not UTF-8 bytes or
grapheme clusters. As with text brackets, negative bounds count from the end,
bounds clamp, and the stop is exclusive. Omission (`alloc.Unset.Unset`) is
not the same as `-1` when stepping backwards. Pass a nonzero literal step or
convert a computed step with `NonZero.try_from(k)?`.

The result is no larger in bytes than the input. Up to seven UTF-8 bytes,
including empty text, fit in the result word without calling `out`. Longer
results request one block from `out` for the header and bytes; even a full
slice copies rather than sharing the input. There is no temporary string or
index buffer. The result outlives both the loan and the allocator handle;
counted copies retain the backing until the last release. An unrepresentable
layout returns `CapacityOverflow`; provider refusal returns its `AllocError`
without changing the input or retrying with system storage. Ordinary `s[a:b:k]?`
and `view[a:b:k]?` still use ordinary storage, regardless of the source.

**Checked serialization:** if the program emits both `slice_text` and a compiled
provider registration, releasing an owning `str` may call a provider with
unknown stack use. The checked rejection bridge takes a `Fault` that may itself
own a string, so even an adapter that simply returns its fault cannot prove
that rejection is safe. See [checked codec stack budgets](serial.md). Text
selection with no compiled provider does not impose that restriction.

**Worker waits:** with both a `slice_text` producer and a compiled provider,
constructing a slice or releasing an owning `str` can call code that waits.
This includes implicit cleanup of string fields. If the worker receives the
allocator used to construct the slice, or can release text from its inputs,
[join and pool checks](../facilities/03-concurrency/channels.md#which-workers-can-wait)
account for mutex types reachable through the provider, even though `str` does
not name the provider. Reading a length through a shared loan does not call it.

### Dictionaries and sets

| Signature | Result on success |
| --- | --- |
| `Allocator.empty_dict[K: KeyType, V](self) -> Result[dict[K, V], AllocError]` | An empty dictionary; no entries are reserved. |
| `Allocator.dict_with_capacity[K: KeyType, V](self, capacity: i64) -> Result[dict[K, V], AllocError]` | An empty dictionary with room for at least `capacity` entries. |
| `Allocator.empty_set[T: KeyType](self) -> Result[set[T], AllocError]` | An empty set; no hash entries are reserved. |
| `Allocator.set_with_capacity[T: KeyType](self, capacity: i64) -> Result[set[T], AllocError]` | An empty hash-backed set with room for at least `capacity` members, or a bitset with room for its whole domain. |

All four methods borrow the allocator. Even an empty owner allocates. For a
hash-backed collection, the object, bookkeeping, rows, insertion-order links
and buckets all use the selected allocator, including later reserve and rehash.
Partial construction releases any storage obtained. If growth fails, existing
entries and their lookup behavior are unchanged. Provider errors propagate as
`AllocError`, without retrying through the system allocator.

Both capacity arguments count entries, not bytes. Zero is valid; a negative or
unrepresentable request returns `AllocError.CapacityOverflow`. For a
[finite-domain bitset](../facilities/01-builtins/sets.md#small-enum-bitsets),
the constructor still checks the ordinary entry-count limit before calling the
provider. A valid request larger than the domain is allowed: the owner allocates
once, with room for every possible member regardless of the hint. Valid reserve
and insertion need no further backing allocation.

The [ordinary collection rules](../facilities/01-builtins/collections.md) apply:
keys and members must satisfy `KeyType`; dictionary values must be heap-storable.
For example, a reference or generator cannot be a stored value, and hidden task
or channel ownership is rejected. Selecting a table does not select storage for
nested values, even a string key constructed from bytes. Neither `copy` nor
ordinary set methods inherit an input's allocator for their result.

### Copy dictionary keys into a set

`Allocator.to_set[K: KeyType](self, view: KeyView[K]) -> Result[set[K], AllocError]`
borrows the allocator and the dictionary behind `view`. Pass a named key view
as `out.to_set(view)` (not `&view`), or project the keys directly as
`out.to_set(d.keys())`, including when `d` is a borrowed dictionary. Only the
keys are copied. They must support explicit copying: affine keys are allowed,
but keys containing custom cleanup are not. Values need not be copyable.

The independent set has one member per dictionary entry. It retains `out`'s
provider for its storage, growth and cleanup; immutable text in a key can keep
its existing backing. If allocation fails, the call releases partial output and
returns the provider's `AllocError` without fallback. Neither success nor failure
changes the dictionary, and a named view remains usable.

Only this trusted method can declare a `KeyView` parameter. User functions
borrow the dictionary and call `keys()` locally; they cannot take or return
the view. For a scoped worker, pass the eligible dictionary and allocator by
shared borrow, not the view as a job argument or capture. The dictionary's
value type still participates in worker eligibility checks.

### Set algebra with an output allocator

| Signature | Members of the result |
| --- | --- |
| `Allocator.union[K: KeyType](self, left: &set[K], right: &set[K]) -> Result[set[K], AllocError]` | In either set |
| `Allocator.intersection[K: KeyType](self, left: &set[K], right: &set[K]) -> Result[set[K], AllocError]` | In both sets |
| `Allocator.difference[K: KeyType](self, left: &set[K], right: &set[K]) -> Result[set[K], AllocError]` | In `left` but not `right` |
| `Allocator.symmetric_difference[K: KeyType](self, left: &set[K], right: &set[K]) -> Result[set[K], AllocError]` | In exactly one set |

A direct call to any of these four `std.alloc` methods accepts a shared set
or a dictionary's keys in either position. For example:

| Operands | Call |
| --- | --- |
| Two sets | `out.union(&a, &b)` |
| Set and key view | `out.intersection(&a, d.keys())` |
| Key view and set | `out.difference(d.keys(), &b)` |
| Two key views | `out.symmetric_difference(d.keys(), other.keys())` |

Both operands must have exactly the same `K`, whether inferred or written as in
`out.difference[str](names, &replied)`. For a local `names = d.keys()`, pass
`names`, not `&names`. The dictionary itself, `d.values()` and `d.items()` are
not algebra operands. Two dictionaries may have different, even noncopyable, value
types: the call probes and copies keys only. Unlike `out.to_set(view)`, which
copies every key into a set, algebra never materializes a view as an input set.
Only keys belonging in the result are copied.

This is a direct-call rule for these trusted methods, not a public overload or
a conversion to `&set[K]`. The declared signatures above still apply to
function values. A key view keeps its whole dictionary borrowed through the
call (until last use for a named view). It cannot be stored in a collection,
passed to or returned from a user function, or captured or passed to a job.
Pass the eligible dictionary itself by shared borrow and project its keys
inside the worker instead; eligibility includes the value type.

`K: KeyType` alone does not guarantee that a key can be duplicated. A legal
affine record key that supports explicit copying works without `@copy`. A key
with custom cleanup (`__del__`), even in a nested field or variant payload, is
rejected at compile time, even if the result would be empty.

The result is an independent `set[K]` that retains `out`'s provider for growth
and cleanup; neither input's provider is selected implicitly. Result storage, any scratch storage and any new allocations needed
to copy keys use that provider. Already shared leaves such as immutable strings
retain their existing backing. A finite-domain bitset needs just the result
owner allocation. If allocation fails, the operation releases partial output
and returns the `AllocError` without fallback. Both inputs remain unchanged on
success and failure.

### Copy with an output allocator

`Allocator.copy[T](self, value: &T) -> Result[T, AllocError]` borrows both
inputs. Use `out.copy(&value)?` to duplicate an existing value without moving
it. When the copy needs a new `Box`, list, dictionary or set, even inside a
record or active enum payload, `out` pays for the owner and its backing. These
owners keep that provider for growth and destruction after `out` is dropped.
Unlike `out.box(value)`, this does not move nested owners with their original
storage. Unlike builtin [`copy(value)`](../facilities/01-builtins/construction-copy.md#explicit-copying),
it chooses the destination explicitly.

Inline fields need no heap allocation of their own. Immutable strings can keep
the same backing; a nested `Shared` handle gains a reference to the same value,
not a copy of its contents. Selection does not migrate those shared leaves.
The walk is bounded by the existing value, not a producer; there is no
separate size parameter. If the provider refuses a request, the copy releases its partial
output and returns the `AllocError` without retrying through the source or
system allocator. The source stays usable on either return.

`T` must satisfy the builtin explicit-copy rules but need not implement
`CopyType`: a class with an owned list field can be copied without `@copy`,
but recursive types, generators, captured closures and records with custom
destruction cannot be duplicated. For a bare `Shared`, use `handle.share()`.

Selected copy additionally requires that `T` cannot carry a loan. For example,
a class with a `data: &[i64]` field is rejected: copying the field would copy
the view, not its backing. Wrapping a borrowed value does not remove its loan.
To copy the elements of a view into selected storage, use
[`out.copy_view(view)`](#copy-a-slice-view-into-a-list).
The ordinary worker borrow and transfer rules and the module's
[actor exclusion](#current-boundaries) still apply.

### Selected boxes and collection

| Signature | Ownership, allocation and errors |
| --- | --- |
| `Allocator.box[T](self, value: T) -> Result[Box[T], BoxError[T]]` | Borrows the allocator and moves `value` into one ordinary `Box[T]` allocation whose storage uses the selected allocator. Failure returns the unchanged value. |
| `Allocator.list[T](self, source: Generator[T]) -> Result[list[T], AllocError]` | Borrows the allocator and consumes `source`, building a new list with the selected allocator. Growth allocates as `append` does; no capacity is reserved from the source. |

`BoxError[T]` has one variant, `Allocation(T, AllocError)`. On failure it returns
the original value unchanged; nothing is moved into the box, and no fallback
allocator is tried.

A successful `Allocator.box` transfers the value into an ordinary concrete
`Box[T]`. Moves, borrows, `*box` extraction and destruction follow the
[boxes](../facilities/01-builtins/boxes.md) rules; the result is not a protocol
object or callable adaptation. The box keeps its allocator, so taking its content
with `*box` or dropping the box releases storage through that allocator after the
caller's handle is gone. Allocations already inside `T` keep their original
allocators. Borrowed values (`&x`) and values with hidden task or channel
ownership are rejected, as ordinary element rules require.

`Allocator.list` is ordinary `for` iteration: it creates an empty selected list,
as `empty_list` does, and appends each yielded element in order. Pass a generator
expression or generator call as the sole argument so `T` is inferred:
`owner.list(a for a in b)?`. Elements are ordinary elements, not flattened: a
generator of `Result[i64, AllocError]` produces `list[Result[i64, AllocError]]`.

The call consumes `source` on every return. On success the list owns every
yielded element. On failure, including one before the first resume, the
generator's captures, the current item not yet inserted and the partial list are
dropped exactly once, and the call returns `Err(AllocError)` with no resumable
generator. Loans held by the captures end when the call returns, so a borrowed
source can be used again afterwards.

The collector has no item limit of its own. The system root is unbounded, so a
source that never ends runs until allocation fails. An explicit provider may
refuse growth with `OutOfMemory`, which bounds the collection. Source termination
and allocations made while the generator produces elements remain the caller's
concern. When the element count is known, `list_with_capacity` reserves it
before appending.

### Custom providers

`Provider` requires public methods with these signatures (substitute the concrete
provider type for `Provider` in the receivers):

```plenty fragment
pub protocol Provider:
    def allocate(self: &mut Provider, request: &Layout) -> Result[ByteBlock, AllocError]:
        pass
    def release(self: &mut Provider, storage: ByteBlock) -> ():
        pass
```

`allocator[P: Provider](bootstrap: &Allocator, provider: P) -> Result[Allocator, RegistrationError[P]]`
borrows the explicit bootstrap and takes the provider by value. Registration
makes exactly **two allocations through that bootstrap**: a selected state box
for `P`, then the retained control allocation. There is no hidden system fallback.
Pre-existing allocations nested in `P` keep their original allocators; registration
does not migrate them.

`RegistrationError[P]` preserves ownership on failure:

- `Allocation(P, AllocError)` returns the original provider unchanged if either
  allocation fails, releasing any provisional storage without destroying `P`.
- `UnsupportedContext(P)` returns it unchanged when a native actor/arena context
  forbids registration, before allocating or transferring ownership. This is a
  runtime guard, not certification of arbitrary foreign provenance.

On success, handles and outstanding owners retain the provider and its bootstrap
storage until final cleanup. Provider callbacks are serialized. The compiler
certifies state, transitive callback effects and destructors for worker use even
when the program never spawns a worker; hidden task/channel ownership is rejected.
This does not relax ordinary element or borrow restrictions.

`allocate` receives the full backing request, including private metadata and
alignment padding, not just the user's payload size. It must return a block with
at least that extent and alignment; oversized blocks are accepted. Insufficient
extent or alignment traps before typed writes. When the backing is no longer
needed, `release` receives that **original full block with every byte zeroed**,
not a zero-length block.
The provider may cache it or drop it. Typed values have already been destroyed;
zeroing also hides padding and stale moved-owner representations.

Returned `AllocError` values remain recoverable without fallback. Traps in
allocation, release, transitive cleanup or final provider destruction are
**process-fatal**. Same-thread callback recursion traps rather than deadlocking;
cyclic dependencies between providers have no deadlock-freedom guarantee.
See the [custom-provider lesson](../../tutorial/04-collections/allocation.md#register-a-custom-provider)
for a minimal forwarding policy.

### Safe byte blocks

| Signature | Behavior |
| --- | --- |
| `Allocator.block(self, layout: &Layout) -> Result[ByteBlock, AllocError]` | Borrows both inputs and allocates exactly the requested zeroed byte range plus private metadata. Even zero size allocates. Metadata/layout overflow returns `CapacityOverflow`; unavailable storage returns `OutOfMemory`. |
| `ByteBlock.len(self) -> i64` | Returns the current range length; splitting changes it. |
| `ByteBlock.is_empty(self) -> bool` | Returns `len() == 0`. |
| `ByteBlock.alignment(self) -> i64` | Returns the actual start-address alignment of the block payload. A new block meets at least the requested alignment; splits may reduce it to one. |
| `ByteBlock.get(self, index: i64) -> u8` | Reads one byte. Index must satisfy `0 <= index < len()`, otherwise named trap. |
| `ByteBlock.set(self: &mut ByteBlock, index: i64, value: u8) -> ()` | Writes one byte. Index must satisfy `0 <= index < len()`, otherwise named trap. |
| `ByteBlock.split_off(self: &mut ByteBlock, at: i64) -> ByteBlock` | Leaves the first `at` bytes in `self` and returns the suffix without allocating or copying bytes. Requires `0 <= at <= len()`, otherwise traps. Both disjoint ranges retain the entire backing, even when empty. |

Invalid positions trap with the operation and argument, such as
`ByteBlock.get: index is outside 0..len()`. Alignment after a split describes
its new start address; splitting an aligned block at an odd offset yields
suffix alignment one.

The list constructors return `AllocError` when storage cannot be obtained.
Partial construction releases successful allocations through their selected
allocator. A failed growth leaves the existing list intact; a failed collection
drops its partial list as described in [selected boxes and collection](#selected-boxes-and-collection).
Element types must be heap-storable and must not hide task/channel ownership
behind erasure.

`Allocator` is affine: move it or explicitly call `share()`, rather than copying
it. Its representation is private. Lists retain the allocator through element
cleanup and storage release. The native system root uses explicit system
allocation rather than ambient actor/arena routing; it has no backing state to
free on final handle drop.

`ByteBlock` cannot be directly constructed, copied or cloned; its representation
is private. Shared read borrows remain supported. Moving or dropping one range
does not invalidate other ranges from the same backing. Only the final range
releases the backing through its retained provenance. `Layout` can be copied,
stored or discarded without allocation.

## Workers

Allocator handles and byte blocks may be moved to workers or shared through
checked borrows. Builtin boxes, lists, columnar lists, dictionaries and sets keep their ordinary
content-dependent worker and borrow rules. For example, a dictionary with
worker-local values cannot be moved to a worker just because its allocator can.
Selection neither adds an allocator type parameter nor permits concurrent mutation.
Byte blocks follow ordinary ownership and borrowing rules, allowing exclusive
write access only through `&mut` and shared read access through `&`.

```plenty
import std.alloc as alloc

def worker(owner: alloc.Allocator, input: i64) -> Result[i64, AllocError]:
    mut values = owner.empty_list[i64]()?
    values.append(input)?
    Ok(len(values))

def main() -> Result[(), AllocError | IoError | SpawnError[()]]:
    owner = alloc.system()
    with spawn(worker, owner.share(), 99).drop_job()? as task:
        print(task.join()?)?
    Ok(())
```
```output
1
```

## Current boundaries

There is no ambient selected allocator or fallback selection in this API.
Ordinary constructors keep their existing paths. Selecting an owner changes
where it obtains its backing, not where the surrounding code allocates. For
example, ordinary `copy(value)`, `view.to_set()`, bracket slices,
sorting scratch space and allocations made while producing elements or boxed
content are not redirected. `out.copy(&value)` selects only the copy itself;
it does not select storage for an expression that constructs `value`.

`ByteBlock` is allocator backing, not a replacement for the general-purpose
[`Bytes`](bytes.md) transport buffer. There is no general typed-storage consumer,
byte-view API, reset arena or raw callback registration here.

Retained allocator use cannot coexist with actor execution, abandonment or heap
operations anywhere in the same lowered program, including unrelated lowered
functions. Importing allocator and actor modules together can therefore be
rejected even without calls from `main`, including programs that only call the
pure `layout` factory. Private runtime binding entry points also reject ambient
actor/arena contexts before allocating or transferring ownership. Trusted separately compiled hosts
must keep retained owners out of actor/arena invocations; the binding checks do
not establish arbitrary foreign provenance or escaping lifetimes.

See [retained allocator storage](../toolchain/02-architecture/allocator-storage.md)
for native contracts and [allocation and traps](../language/08-errors/02-allocation-and-traps.md)
for recoverable allocation errors and callback traps.

## Acknowledgements

- [Zig](https://ziglang.org/documentation/master/#Allocator) inspired explicit
  allocator passing. No Zig code is included.
