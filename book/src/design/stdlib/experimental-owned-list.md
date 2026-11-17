# Experimental allocator-backed owned lists

[`std.experimental.alloc`](experimental-alloc.md) supplies `OwnedList[T, P]`
as the B13 generic-ownership gate. Its elements occupy storage returned by an
explicit `BlockProvider`; its provider is retained by value for growth and
release. This is not builtin `list[T]` or the final allocator representation.

## API

| API | Contract |
| --- | --- |
| `owned_list[T, P: BlockProvider](provider: P) -> OwnedList[T, P]` | Moves the provider into an empty owner without allocating. |
| `len() -> i64`, `is_empty() -> bool`, `capacity() -> i64` | Queries without allocation. |
| `reserve(capacity: i64) -> Result[(), AllocError]` | Requests total capacity. Negative or unrepresentable capacities fail with `CapacityOverflow`; smaller valid capacities do not shrink storage. |
| `append(value: T) -> Result[(), AppendFailure[T]]` | Moves the value into available capacity or grows from zero to four slots, then doubles. On failure, returns the unchanged input in public field `value`, with the allocation error in public field `error`. |
| `pop() -> Option[T]` | Moves the final value out without allocating; empty owners return `Nothing`. |

Mutating operations require an exclusive reference to the owner. It cannot be
copied or transferred to workers. There is no iterator, indexing, borrowed
item access or implicit conversion to a builtin list yet.

```plenty
import std.experimental.alloc as memory

def main() -> Result[(), AllocError | memory.AppendFailure[str] | IoError]:
    system = memory.system()?
    mut values = memory.owned_list[str, memory.Allocator](system.share())
    values.append(("owned" + " text")?)?
    values.reserve(12)?
    drop(system)
    print(values.len(), values.capacity())?
    text = values.pop().unwrap()
    drop(values)
    print(text)?
    Ok(())
```
```output
1 12
owned text
```

The string expression uses its existing builtin allocation path; choosing the
list's provider does not relocate nested allocations. Callers needing to retain
an input on failure match `Err(failure)` and recover `failure.value`. Propagating
that error transports ownership of the input, not just an error code.

## Growth, destruction and provider lifetime

`BlockProvider.allocate` supplies a genuine block of at least the requested
extent. `OwnedList` checks its extent and eight-byte alignment **before** typed
access; a broken provider contract traps with a message naming
`OwnedList.reserve`. No fallback provider is used.

Growth allocates first. Allocation failure leaves existing length, capacity and
values unchanged. After success, private compiler operations move initialized
slots into the replacement, relocating nested inline payloads and clearing the
old slots. They neither retain nor destroy moved values. The emptied old block
is passed to `BlockProvider.release(self: &mut BlockProvider, block: ByteBlock)`.
That infallible callback may cache the owned block or drop it. It must not expose
uninitialized bytes; all ByteBlock operations remain checked.

Destruction pops and destroys remaining values in reverse insertion order, then
returns the emptied buffer through `release`, then destroys the provider through
ordinary field cleanup. Providers therefore remain alive through element
cleanup and buffer release. The example in `examples/allocators/owned_values.plenty`
exercises an external bump provider, nested owned byte blocks, growth, pop and
exactly-once custom destructors. Tests also exercise a caching provider that
reuses returned storage and inspect that returned slots are cleared.

Directly held `ByteBlock` values retain only backing storage, not a custom
provider callback. The generic collection supplies the automatic provider
lifetime/release protocol. The system allocator and example bump provider
implement `release` by dropping the block; bump space is not reclaimed.

## Storage boundary and limits

The compiler's private slot ABI is used, not C layouts: a 16-byte value word
plus any owner-local inline payload, with eight-byte storage alignment. Private
std-only operations compute slot size, move a value into a slot, and take it
back out. No public cast from bytes to typed values exists. Typed blocks stay
private while they hold initialized values; users cannot split or mutate their
bytes. A zero-length `array[T, 0]` field keeps `T` visible to type analysis.

Ordinary heap-storable values, including unit, strings, arrays, enums, nested
collections and classes with destructors, are supported. Borrowed values and
hidden task/channel ownership are rejected; this probe must not bypass existing
lifetime, join or shutdown checks. Capacity is bounded by representable slot
bytes and the selected provider's allocation limit, with the same growing-owner
exception to input caps as `IntList`.

No stable allocator ABI, public raw access, higher alignment, arena reset,
concurrent sharing or builtin allocation migration is implied. Internal slot
layout may change with the compiler. Remaining implementation work is tracked
in [B13](../../backlog.md#b13-explicit-allocator-implementation-sequence).
