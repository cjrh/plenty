# Boxed callable environments

Three owner types erase a callback's producer behind a structural signature.
They differ only in what a call may do to the environment:

| Owner | A call | Accepts |
|---|---|---|
| `BoxedCallable[[inputs], result]` | borrows the box | named functions, shared-call closures |
| `BoxedMutCallable[[inputs], result]` | borrows the box mutably | the above, and mutable closures |
| `BoxedOnceCallable[[inputs], result]` | consumes the box | the above, and one-shot closures |

Each mode accepts every callback whose own call needs no more than the mode
grants. The rest of this page describes `BoxedCallable`; the
[mutable and one-shot modes](#mutable-and-one-shot-modes) follow the same rules
except where stated.

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

`BoxedCallable` accepts reusable shared-call callbacks with wholly owned
captures. Borrowed captures, generator-containing environments, and callbacks
returning references are rejected; a mutable closure is rejected with a
suggestion to use `BoxedMutCallable`, and a one-shot closure with one to use
`BoxedOnceCallable`. Signatures otherwise use the same rules as `Callable`,
including reference parameters and ordinary owned arguments, with at most 255
parameters plus the hidden environment slot. A `BoxedCallable` or
`BoxedMutCallable` satisfies a matching generic `Callable` constraint, and every
mode satisfies `OnceCallable`; the call permission and affine ownership remain
visible to specialization.

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
callbacks of every mode, including nested fields and container elements, cannot
cross worker or channel boundaries. Retain a concrete environment when worker
eligibility must be checked.

## Mutable and one-shot modes

`BoxedMutCallable(callback)` and `BoxedOnceCallable(callback)` construct like
`BoxedCallable`: each consumes the callback, attempts one allocation and returns
`Result[..., AllocError]`, dropping the callback on failure. They reject the
same borrowed captures, generator-containing environments and reference
results. The three owner types are distinct: no mode converts to another.
A generic consumer with `F: Callable[...]` taking `&mut F` accepts a
`BoxedMutCallable`; one taking `&F` is rejected at instantiation because the
call needs a mutable borrow. A `BoxedOnceCallable` satisfies only
`F: OnceCallable[...]` with an owned `F`.

A `BoxedMutCallable` call borrows its owner mutably, so the binding must be
`mut` or the owner reached through `&mut`, for example
`for callback in &mut callbacks: callback()?`. Each box keeps its own captured
state, so two boxes from repeated calls to one factory count independently.
A shared reference cannot call it.

A `BoxedOnceCallable` call consumes its owner. Call an owned binding or a
temporary; extract a stored job first, with `pop`, consuming iteration
(`for job in jobs: job()?`), matching or unpacking. A reference cannot call it.
The call moves the environment out of the box, frees the box, and runs the
callback, which drops its remaining captures. A reusable closure in a one-shot
box runs once and its captures drop after the call returns. An uncalled box
drops its captures when its owner is dropped. Either way each capture is
dropped exactly once and the call allocates nothing.

```plenty
def counter(step: i64) -> Result[BoxedMutCallable[[], Result[i64, OverflowError[i64]]], AllocError]:
    mut total = 0
    tick = def [mut total, step]() -> Result[i64, OverflowError[i64]]:
        total = total + step
        Ok(total)
    BoxedMutCallable(tick)

def greet(name: str) -> Result[BoxedOnceCallable[[], Result[(), AllocError | IoError]], AllocError]:
    job = def once [name]() -> Result[(), AllocError | IoError]:
        print(f"hello {name}"?)
    BoxedOnceCallable(job)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut handlers = [counter(1)?, counter(10)?]?
    for handler in &mut handlers:
        print(handler()?)?
    for handler in &mut handlers:
        print(handler()?)?
    jobs = [greet("ada")?, greet("lin")?]?
    for job in jobs:
        job()?
    Ok(())
```

```output
1
10
2
20
hello ada
hello lin
```

## Choosing a callback type

Each callback kind can appear in these positions:

| Kind | Parameter | Return | Field, tuple, list, enum | Worker or channel |
|---|---|---|---|---|
| Named function, capture-free closure | `Callable[...]`, or generic `F` | `Callable[...]` | yes, as `Callable[...]` | yes, certified against every [matching address](../../facilities/03-concurrency/effects.md#thin-callable-targets) |
| Owned shared-call closure | generic `F`, by `&F` | `Closure[...]` | yes; a list holds one producer | as a [concrete job](../../facilities/03-concurrency/jobs.md) |
| Owned mutable closure | generic `F`, by `&mut F` | `Closure[...]` | yes; a list holds one producer | as a concrete job |
| Owned one-shot closure | generic `F: OnceCallable[...]`, by value | `OnceClosure[...]` | yes; extract it before the call | as a concrete job |
| Borrowing closure | generic `F`, by reference | no | no | no |
| `BoxedCallable` | its type, or generic `F`, by `&F` | its type | yes, any producers | no |
| `BoxedMutCallable` | its type, by `&mut` | its type | yes, any producers | no |
| `BoxedOnceCallable` | its type, by value | its type | yes, any producers; extract it before the call | no |

A concrete call dispatches directly, inlines when profitable and allocates
nothing. A boxed call is one indirect call through its adapter, which knows the
concrete type; constructing the box is the only allocation, and dropping it
frees that allocation once.

A generic consumer accepts every kind, including borrowing closures, without
allocating, so prefer it for helpers that only call a callback. A concrete
closure type keeps its producer: a `Closure` or `OnceClosure` annotation is
inferred from the closure expression a function returns, every return path
must use the same producer, and a list holds a single producer. When a type
mismatch names two closure definitions with the same signature, the
diagnostic suggests boxing each in the mode they need. Borrowing captures in
erased storage remain [backlog item B32](../../../backlog.md#scope-of-follow-on-work).
