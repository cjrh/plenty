# Shared owners

`Shared[T]` is one heap value with several owners. Each owner is an affine
handle; the value is released when the last handle drops, on whichever thread
drops it. Use it to give pool jobs, or any owners that cannot borrow from one
scope, the same atomic, mutex, or immutable value. It is a prelude name, like
[`Box`](boxes.md).

| Operation | Result | Effect |
|---|---|---|
| `Shared[T](value)`, `Shared(value)` | `Result[Shared[T], AllocError]` | Move `value` into one new allocation |
| `handle.share()` | `Shared[T]` | Another owner of the same value |
| `handle.field`, `handle.method(...)`, `handle[i]` | as for `T` | Use the content through a shared borrow |
| `&*handle` | `&T` | Borrow the content |
| `*handle` | `T` | Copy copyable content |
| `with handle.lock() as value:` | binds `value: &mut V` | Lock a `Shared[Mutex[V]]`; see [mutexes](../03-concurrency/mutex.md) |

Construction makes one allocation, a counted box; failure drops the input and
returns `AllocError`. `share()` increments the count and allocates nothing.
Dropping a handle decrements it; the last drop runs `T`'s destructor once and
frees the allocation. The count is atomic, so handles may be moved into, shared
with, and dropped on any [worker](../03-concurrency/index.md).

## Access

A handle gives only shared access to its content. Every access borrows the
content through `*handle`, and that loan is rooted at the handle's binding:
while a field reference, element reference, or `&*handle` is live, the handle
cannot be moved, dropped, or reassigned. The handle may be a `&Shared[T]`
reference; `share()` through it makes a new owner.

The content changes only through types that synchronize their own mutation:
an [`Atomic`](../../stdlib/atomic.md), a [`Mutex`](../03-concurrency/mutex.md),
a channel endpoint, or a cancellation token. `&mut *handle`, assignment to a
field or element, `&mut self` methods, and mutating collection methods are
rejected. `*handle` copies copyable content; moving affine content out is
rejected, since other owners may still use it. There is no implicit conversion
from `Shared[T]` to `T` or from `&Shared[T]` to `&T`; write `&*handle`.

A temporary handle, such as `Shared(5)?.share()`, has no binding to root a
borrow at and must be named first.

## Rules

- Handles have no equality, formatting, `print`, or `copy()`, including when
  stored inside other values. Compare or print values read through the handle.
- `T` follows the [storage rules](../../language/05-ownership/index.md) of a
  box: no references, generators, or borrowed environments.
- `T` cannot reach a `ThreadPoolExecutor` or `Future`. The last owner could
  drop on one of the pool's own workers, which would then wait for its own pool.
- `T` cannot reach `Shared[T]` again through a `Mutex` or a channel endpoint.
  Their contents change after construction, so a value could come to hold a
  share of itself and never be released. Immutable sharing, as in a tree or DAG
  of `Shared` nodes, is accepted: each node exists before any share of it. Link
  back to a parent or a sibling with an index into an owning list instead; see
  [recursive data](../../language/03-types/recursive-data.md). There is no
  weak handle. For the same reason a `BoxedCallable` cannot be stored behind
  such a mutex or channel: its captures are not visible to the check.

Rules are checked at each construction, with the concrete `T`.

## Threads

A handle crossing a thread boundary is checked like any other value: its
content must be [worker-eligible](../03-concurrency/index.md). Because the last
owner can drop on any holder's thread, the content's destructors are checked as
if every holder could run them. Mutexes reachable through a handle count for the
compiler's lock checks: a [join, result wait, or pool shutdown](../03-concurrency/mutex.md#waits-the-compiler-rejects)
while holding a lock on a `Mutex[V]` that a worker can reach through a share is
rejected, and so is nesting two locks of the same `Mutex[V]` type, since two
handles can name one mutex.

## Lowering

A handle is a pointer to the same counted record as a box: a header with an
atomic reference count and destructor, the content's type, and the content
slot. `share()` is a relaxed atomic increment; a release decrements with
release ordering, and the last owner acquires before destroying. Borrowing the
content is an address computation. See the
[memory architecture](../../toolchain/02-architecture/memory.md).
