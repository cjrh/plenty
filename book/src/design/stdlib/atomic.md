# `std.atomic`

Inline atomic integers and booleans that scoped workers update through shared
references. Exports `Atomic[T]`, `Ordering`, and `fence`. They are not prelude
names: each use needs an import, through any item or module alias. Supported
target: x86_64 Linux GNU.

```plenty
from std.atomic import Atomic, Ordering

def work(hits: &Atomic[u64]) -> ():
    for _ in range(1000):
        hits.fetch_add(1, Ordering.Relaxed)

def main() -> Result[(), ThreadError | IoError]:
    hits = Atomic[u64](0)
    with spawn(work, &hits)?, spawn(work, &hits)?:
        pass
    print(hits.load(Ordering.Acquire))?
    Ok(())
```
```output
2000
```

## `Atomic[T]`

`T` is `bool`, `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, or `u64`; any
other element type is a compile-time error. `Atomic[T](value)` returns an
`Atomic[T]` directly. `Atomic(value)` takes `T` from the expected type, such
as an annotation or a field, or else from the value.

An atomic is stored inline wherever its owner stores values: a local, a class
field, a tuple or enum payload, or a collection element. Construction and every
operation allocate nothing and call no runtime function.

An atomic is affine: it moves and cannot be copied. It has no equality, no
ordering, and no formatting; read its value with `load`. `*r` cannot read it and
`*r = ...` cannot replace it. Moving or reassigning an unborrowed atomic is
ordinary.

Methods take the receiver by shared borrow, so a `&Atomic[T]` given to several
workers can update it. The receiver must be a place: a name, field, element, or
`*` dereference. An operation on a temporary is rejected, except `into_inner`.

| Method | Result | Notes |
|---|---|---|
| `load(order)` | `T` | |
| `store(value, order)` | `()` | |
| `swap(value, order)` | `T` | previous value |
| `compare_exchange(current, new, success, failure)` | `Result[T, T]` | `Ok(previous)` when it equaled `current` and was replaced, else `Err(actual)` |
| `fetch_add(value, order)`, `fetch_sub` | `T` | integer only; overflow traps before storing |
| `fetch_wrapping_add(value, order)`, `fetch_wrapping_sub` | `T` | integer only; wraps |
| `fetch_and`, `fetch_or`, `fetch_xor`, `fetch_nand` | `T` | |
| `fetch_max(value, order)`, `fetch_min` | `T` | integer only; compares by `T`'s signedness |
| `fetch_not(order)` | `bool` | `bool` only |
| `into_inner()` | `T` | consumes an owned atomic |

Every `fetch_` method returns the previous value. `fetch_add` and `fetch_sub`
use the ordinary `integer overflow` trap at the call's position. They compute
the checked result from the value last read and store it with a
compare-exchange, retrying if another thread changed the value, so no thread
observes a wrapped value. They trap on the value they read even if another thread
changes it afterwards: that is a valid ordering of the operations, and it keeps
the guarantee that no wrapped value is stored, unlike Rust's wrapping
`fetch_add`. The other methods lower to one atomic instruction,
or a compare-exchange loop where the target has none. No operation blocks or
takes a lock; a compare-exchange loop retries while other threads keep changing
the value, so it is lock-free but not wait-free.

## `Ordering`

`Ordering` is an enum: `Relaxed`, `Release`, `Acquire`, `AcqRel`, `SeqCst`.
Each ordering argument is written as `Ordering.<variant>`, through any alias. A
variable, call, or other expression is a compile-time error, as is an ordering
the operation does not allow:

| Operation | Allowed orderings |
|---|---|
| `load` | `Relaxed`, `Acquire`, `SeqCst` |
| `store` | `Relaxed`, `Release`, `SeqCst` |
| Read-modify-write methods | all five |
| `compare_exchange` success | all five |
| `compare_exchange` failure | `Relaxed`, `Acquire`, `SeqCst` |
| `fence` | `Release`, `Acquire`, `AcqRel`, `SeqCst` |

The orderings mean what they mean in C++ and Rust. Every operation currently
compiles to a sequentially consistent instruction, which is at least as strong
as any requested ordering, so a program cannot rely on a weaker ordering being
weaker. `Ordering` values can also be stored and matched like any enum value.

## `fence`

`fence(order) -> ()` orders memory operations around it without an atomic
location. It compiles to a full sequentially consistent fence.

## Sharing

Scoped threads borrow an atomic, or a class holding atomics, for the task's
scope. The parent may keep calling methods on it while the workers run. Executor
jobs own their inputs, so they share an atomic through a
[`Shared[Atomic[T]]`](../facilities/01-builtins/shared.md) handle: each job
receives its own `handle.share()` and calls methods on it directly.

Not provided: `compare_exchange_weak` (identical to `compare_exchange` on the
supported target), `fetch_update` (write a `compare_exchange` loop),
`get_mut`, `as_ptr`, compiler-only fences, and pointer atomics.

For a lesson see [count with atomics](../../tutorial/11-concurrency/atomic-counters.md);
the [architecture page](../toolchain/02-architecture/concurrency.md#atomics)
describes the lowering.

## Acknowledgements

- Operations and orderings follow Rust's [`std::sync::atomic`](https://doc.rust-lang.org/std/sync/atomic/).
