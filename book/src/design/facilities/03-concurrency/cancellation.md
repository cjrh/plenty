# Cancellation tokens

`CancellationToken() -> Result[CancellationToken, AllocError]` creates an affine
owner of a shared cancellation state using one fallible allocation. Tokens may
cross threads and live in owned containers; assignment moves them. Copy and
equality are unsupported.

| Operation | Result | Behavior |
| --- | --- | --- |
| `token.share()` | `CancellationToken` | Another owner of the same state |
| `token.cancel()` | `()` | Permanently request cancellation and wake all token waiters |
| `token.is_cancelled()` | `bool` | Observe the request |
| `token.wait()` | `()` | Wait until requested |
| `token.wait_timeout(timeout_ms: u64)` | `bool` | True if requested within the millisecond budget |
| `with token.cancel_on_exit():` | guard | Cancel on every exit from the block; see [below](#cancel-on-every-exit) |

All methods observe their receiver without requiring `mut` and allocate nothing.
Cancellation is idempotent. Dropping a handle does not request cancellation;
the last owner releases the state. Timeout does not consume or reset it. `wait_timeout` is also the cancellable
sleep; [`std.time`](../../stdlib/time.md#sleeping-and-timed-waits) supplies its
budget from a `Duration`.

Workers must check the token at safe boundaries, finish required cleanup, and
return normally. A token imposes no return type or error variant. It does not
change future state by itself: [Future.cancel](futures.md) only prevents pending
work from starting.

Ordinary blocking channel operations and foreign calls do not observe tokens.
Use [timed waits](timed-selection.md) or an explicit shutdown message when work
must periodically regain control. Scope exit and executor shutdown still join
running workers, so cancellation-aware code must actually return, and the code
that cancels must reach its cancel on every exit; see
[cancel on every exit](#cancel-on-every-exit).

## Cancel on every exit

A task or pool job that waits for a token keeps its join, or its pool's
shutdown, waiting until someone cancels the token. `token.cancel_on_exit()` is a
`with` guard that cancels the token on every exit from its block: its end,
`return`, `?`, `break`, `continue`, and the failed start of a later manager in
the same statement. It holds a share of the token, allocates nothing, has no
value to bind with `as`, and is valid only as a `with` manager.

Contexts exit in reverse order, so put the guard directly after each task or
pool that holds the token:

```text
with spawn(serve, listener, stop.share())?, stop.cancel_on_exit():
    ...
with ThreadPoolExecutor(2, 8)? as pool, stop.cancel_on_exit():
    ...
```

To join inside the block, or to wait for a job's `result()`, cancel first:
either call `stop.cancel()` before the wait, or end a nested guard before it:

```text
with spawn(serve, stop.share())? as task:
    with stop.cancel_on_exit():
        ...
    task.join()
```

A worker that cancels a token when it finishes, so that its caller can stop
waiting, puts its body in `with stop.cancel_on_exit():`.

### Waits the compiler rejects

The compiler rejects a wait that could skip the function's own later cancel:

> When a function cancels a token binding, a join of a task, a `result()` of a
> job, or a pool shutdown that waits for work holding that token must come after
> a cancel on every path that reaches it.

A task or job holds the token when its arguments move, share or borrow that
binding, including a `share()` of it bound to another name. The rule covers
`task.join()`, `future.result()`, `drop(pool)`, and the automatic join or
shutdown on every exit listed above. A guard placed before the task in the
header is rejected too, since it cancels after the join. The diagnostic names
the exit, the job's start where it helps, and the later cancel, and suggests
the guard.

A worker is held to its own final cancel: when a function's normal end always
cancels a token parameter, an earlier `return` or `?` that skips the cancel is
rejected. A conditional cancel is not checked.

### What the check does not decide

The check is local to one function and linear in its size. It does not see:

- tokens reached through a field, a `Shared[T]`, a class value built before the
  start, or a closure capture;
- the tokens that `map`, `map_result` or `reduce_tree` jobs hold;
- a cancel made by a called function;
- a worker's deadline or timed wait, so a worker that stops by itself is still
  checked;
- which pool runs which job: a function-exit shutdown counts every owned pool;
- every path through a loop: a loop body is walked at most twice.

A program outside these forms can still wait forever; the guard is the habit
that avoids it.
