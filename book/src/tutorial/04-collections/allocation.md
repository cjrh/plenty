# Handle allocation failures

Collection construction returns `Result[collection, AllocError]`. Use `?` to
propagate a failure, or `match` to recover:

```plenty
def answers() -> Result[list[i64], AllocError]:
    mut values = list[i64].with_capacity(2)?
    values.append(21)?
    values.append(42)?
    Ok(values)

def main() -> Result[(), IoError]:
    match answers():
        case Ok(values):
            print(values)?
        case Err(AllocError.OutOfMemory):
            print("not enough memory")?
        case Err(AllocError.CapacityOverflow):
            print("requested capacity is too large")?
    Ok(())
```

```output
[21, 42]
```

`list[i64].with_capacity(2)?` constructs an empty list with room for at least
two elements. Appending those elements needs no further list storage allocation.
Use `list[i64]()` when you do not know the capacity ahead of time.
`dict[str, i64]()` and `set[str]()`
construct the other collection kinds. Displays such as `[21, 42]` return the
same kind of allocation result; extract their collection with `?` too.
List displays reserve their full element count automatically. Both mutable and
immutable list bindings benefit. Element expressions may still allocate.

For an existing mutable collection, `values.reserve(n)?` reserves room for
`n` **additional** elements beyond its current length. Existing contents remain
unchanged if reservation fails.

Growth also returns a result: `append` for lists, `insert` for dictionaries,
and `add` for sets. A failed insertion keeps existing contents but consumes
its arguments, cleaning up any owned argument rather than returning it.

`AllocError` has two payload-free variants. `OutOfMemory` means the
allocator rejected a request. `CapacityOverflow` means its size cannot be
represented. Neither variant needs allocation.

## Select collection storage explicitly

With [`std.alloc`](../../design/stdlib/alloc.md), you can choose a system or
custom allocator when you construct a collection. The collection keeps that
allocator for growth and cleanup, even after the handle is dropped:

```plenty
import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    mut values = owner.list_with_capacity[i64](2)?
    drop(owner)
    values.append(21)?
    values.append(42)?
    print(values)?
    Ok(())
```
```output
[21, 42]
```

The same choice is available for dictionaries and sets. Here the dictionary
reserves space for two scores. The set of absent players starts empty and
obtains its hash storage on demand:

```plenty
import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    owner = alloc.system()
    mut scores = owner.dict_with_capacity[str, i64](2)?
    mut absent = owner.empty_set[str]()?
    drop(owner)
    scores.insert("Ada", 21)?
    scores.insert("Lin", 42)?
    absent.add("Jo")?
    print(scores["Lin"], "Jo" in absent)?
    Ok(())
```
```output
42 True
```

Use `empty_dict` if the number of keys is unknown, or `set_with_capacity` to
reserve for a known number of members. These constructors select the table's
storage, not the storage used to build a string key. See the
[dictionary and set allocation contract](../../design/stdlib/alloc.md#dictionaries-and-sets)
for capacity errors and the fixed-size bitset case.

## Choose storage for a set result

Suppose a guest list maps each invitee to their party size. To find invitees
who haven't replied, compare the dictionary's keys with the set of replies.
Call `difference` on the allocator that should pay for the result:

```plenty
import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    invited = {"Ada": 2, "Lin": 1}?
    replied = {"Lin"}?
    out = alloc.system()
    pending = out.difference(invited.keys(), &replied)?
    print("Ada" in pending, "Lin" in pending)?
    print(len(invited), len(replied))?
    Ok(())
```
```output
True False
2 1
```

`invited.keys()` borrows the names in place. There's no intermediate set of
invitees, and the party sizes are never copied. Both inputs stay unchanged
even if allocation fails. The same form works for `union`, `intersection` and
`symmetric_difference`. See
[set algebra with an output allocator](../../design/stdlib/alloc.md#set-algebra-with-an-output-allocator)
for key-copy requirements.

## Keep the player names without copying the scores

If all you need is a membership check now, use `name in scores` without
allocating. To keep the names after the scores change, borrow the dictionary
and let the caller choose storage for an independent set:

```plenty
import std.alloc as alloc

def player_names(scores: &dict[str, i64], out: &alloc.Allocator) -> Result[set[str], AllocError]:
    out.to_set(scores.keys())

def main() -> Result[(), AllocError | IoError]:
    mut scores = {"Ada": 21, "Lin": 42}?
    out = alloc.system()
    names = player_names(&scores, &out)?
    scores.clear()
    print("Ada" in names, len(names), len(scores))?
    Ok(())
```
```output
True 2 0
```

Only the keys are copied, so this also works when the scores are replaced by
values that cannot be copied. If you already have `view = scores.keys()`, use
`out.to_set(view)?`. A failure leaves `scores` unchanged. See the
[key conversion contract](../../design/stdlib/alloc.md#copy-dictionary-keys-into-a-set).

## Copy a value into selected storage

Suppose you need a working copy of a score sheet. Call `copy` on the allocator
that should pay for it. The sheet itself is a record; its list needs new storage
so edits to the copy leave the original alone:

```plenty
import std.alloc as alloc

class Sheet:
    scores: list[i64]

def main() -> Result[(), AllocError | IoError]:
    original = Sheet([21, 42]?)
    out = alloc.system()
    mut working = out.copy(&original)?
    drop(out)
    working.scores.append(63)?
    print(original.scores)?
    print(working.scores)?
    Ok(())
```
```output
[21, 42]
[21, 42, 63]
```

If allocation fails, `?` propagates the error; the call itself does not consume
`original` even then. Text and nested `Shared` handles keep their existing
backing. See [copy with an output allocator](../../design/stdlib/alloc.md#copy-with-an-output-allocator)
for which values can be copied.

## Copy selected list elements

To keep a reversed score list in storage chosen by the caller, use `out.slice`.
It borrows the scores; the result can outlive them:

```plenty
import std.alloc as alloc

def main() -> Result[(), AllocError | IoError]:
    scores = [70, 85, 90]?
    out = alloc.system()
    reversed_scores = out.slice(&scores, step=-1)?
    print(out.slice(&scores, stop=-1, step=-1)?)?
    drop(scores)
    print(reversed_scores)?
    Ok(())
```
```output
[]
[90, 85, 70]
```

Both calls start at the last score. With `stop=-1`, that same score is the
exclusive stop, so nothing is copied. Omitting `stop` lets the reverse walk
include the first score. If you just need to read adjacent scores in place,
borrow `&scores[a:b]` instead. See the [list slice allocation contract](../../design/stdlib/alloc.md#copy-a-list-slice-with-an-output-allocator).

## Keep scores from a borrowed view

A helper that saves a run of scores doesn't need the whole list or the
caller's indices. Take a view of just those scores and use `copy_view` to
keep them in storage the caller chooses:

```plenty
import std.alloc as alloc

def save_scores(out: &alloc.Allocator, scores: &[i64]) -> Result[list[i64], AllocError]:
    out.copy_view(scores)

def main() -> Result[(), AllocError | IoError]:
    out = alloc.system()
    mut scores = [70, 85, 90, 75]?
    saved = save_scores(&out, &scores[1:3])?
    scores[1] = 100
    print(saved, scores)?
    Ok(())
```
```output
[85, 90] [70, 100, 90, 75]
```

`&scores[1:3]` borrows without allocating. Only `copy_view` makes a new list,
so the saved scores stay the same when the caller updates the original. For a
helper that just reads the scores, use the view directly and skip the copy.
See [copy a slice view into a list](../../design/stdlib/alloc.md#copy-a-slice-view-into-a-list).

## Box a value and collect a generator

`owner.box(value)` moves a value into an ordinary `Box[T]` allocated through the
selected allocator. `owner.list(source)` consumes a generator and returns a list
that keeps the allocator. Both report allocation failures through `?`:

```plenty
import std.alloc as alloc

def main() -> Result[(), alloc.BoxError[i64] | AllocError | IoError]:
    owner = alloc.system()
    mut boxed = owner.box(41)?
    reference = &mut *boxed
    *reference = 42
    value = *boxed
    print(value)?
    values = owner.list(index for index in range(3))?
    drop(owner)
    print(values)?
    Ok(())
```
```output
42
[0, 1, 2]
```

A mutable borrow such as `&mut *boxed` edits the content in place. Taking it with
`*boxed` moves the content out and releases the box's storage through its
retained allocator.

If boxing fails, `BoxError.Allocation` returns the original value unchanged, so
the caller can keep it or retry elsewhere rather than losing it.

The generator expression yields each index in order; see
[generators](../../design/language/09-iteration/03-generators.md).
The collector infers its element type from the generator and does not reserve
capacity from the source. When the count is known, reserve it with
`list_with_capacity` and append, as the
[collection storage example](#select-collection-storage-explicitly) shows.
The collector does not cap its input, so bound a generator that could run long
before collecting it. A failed collection consumes the generator too: its
captured values and partial list are dropped once, and the call returns the
`AllocError`.

## Allocate backing blocks

For allocator backing storage, `layout` validates a byte size and alignment
without allocating. `block` allocates zeroed bytes; even zero-size blocks allocate
metadata. Use [`Bytes`](../../design/stdlib/bytes.md) for ordinary byte I/O.

```plenty
import std.alloc as alloc

def main() -> Result[(), alloc.LayoutError | AllocError | IoError]:
    owner = alloc.system()
    request = alloc.layout(10)?
    mut block = owner.block(&request)?
    mut tail = block.split_off(5)
    drop(owner)
    tail.set(0, 42u8)
    print(block.len(), block.get(0), tail.get(0))?
    Ok(())
```
```output
5 0 42
```

Splitting transfers disjoint ranges without allocation or copying. Each range,
including an empty one, retains the full backing until the last range is dropped.
Blocks can be borrowed for reading, but cannot be copied into overlapping owners.

## Register a custom provider

A provider supplies backing blocks and receives them back for reuse or disposal.
This minimal forwarding policy delegates to a separate backing handle. Its public
methods satisfy `alloc.Provider`; no raw pointers or private hooks are needed.
The bootstrap pays for registration, while `backing` supplies later requests:

```plenty
import std.alloc as alloc

pub class Forwarding:
    backing: alloc.Allocator

    pub def allocate(self: &mut Forwarding, request: &alloc.Layout) -> Result[alloc.ByteBlock, AllocError]:
        self.backing.block(request)

    pub def release(self: &mut Forwarding, storage: alloc.ByteBlock) -> ():
        drop(storage)

def main() -> Result[(), alloc.RegistrationError[Forwarding] | AllocError | IoError]:
    bootstrap = alloc.system()
    backing = alloc.system()
    owner = alloc.allocator(&bootstrap, Forwarding(backing))?
    mut values = owner.list_with_capacity[i64](2)?
    drop(bootstrap)
    drop(owner)
    values.append(21)?
    values.append(42)?
    print(values)?
    Ok(())
```
```output
[21, 42]
```

Use the system handle directly when forwarding adds no policy; this example
isolates the provider contract rather than offering an allocation optimization.
Registration makes two allocations through the explicit bootstrap (state box
and control allocation), with no hidden fallback. Either failure returns the
unchanged provider in `RegistrationError.Allocation`; `?` preserves that complete
error here. A native unsupported context returns `UnsupportedContext` with the
provider. Success lets lists and blocks outlive both initiating handles.
Pre-existing allocations inside the provider are not redirected.

The request includes private metadata and padding. `release` receives the full
original backing with all bytes zeroed, so a caching policy can safely reuse it.
Callbacks serialize, and the compiler checks worker-safe state, effects and
cleanup even without `spawn`. Return allocation errors rather than trapping:
callback traps, including cleanup traps, terminate the process.

## Current limits

Retained allocator operations cannot coexist with actor operations anywhere in
the lowered program, including unused imports and layout-only use. Selection
does not redirect element expressions, builtin `copy`, `view.to_set()`, ordinary
set methods or bracket slices to an input's allocator. The ordinary rules for storing, borrowing and moving
values to workers still apply. Ordinary constructors remain available by design. Use `xs[1:4]?` for a new
list with ordinary system storage, or `out.slice(&xs, 1, 4)?` to charge the
copy to `out`. Both can fail. If `out` refuses, the copy does not retry with
system storage. Appending to an existing list uses that list’s allocator.
Resettable arenas are not part of this interface.

Use [capacity and reservation](../../design/examples/collections/capacity.md)
to obtain storage before inserting values you want to keep if reservation
fails. For byte-oriented allocator policies, see the safe-block section of
[`std.alloc`](../../design/stdlib/alloc.md#safe-byte-blocks).
