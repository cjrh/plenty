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

All methods observe their receiver without requiring `mut` and allocate nothing.
Cancellation is idempotent. Dropping a handle does not request cancellation;
the last owner releases the state. Timeout does not consume or reset it.

Workers must check the token at safe boundaries, finish required cleanup, and
return normally. A token imposes no return type or error variant. It does not
change future state by itself: [Future.cancel](futures.md) only prevents pending
work from starting.

Ordinary blocking channel operations and foreign calls do not observe tokens.
Use [timed waits](timed-selection.md) or an explicit shutdown message when work
must periodically regain control. Scope exit and executor shutdown still join
running workers, so cancellation-aware code must actually return.
