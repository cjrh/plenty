# Thread-pool executors

The API takes its naming and context-manager shape from
[Python's concurrent.futures](https://docs.python.org/3/library/concurrent.futures.html).
The ownership, allocation, and shutdown guarantees below define Plenty's behavior.

`ThreadPoolExecutor(max_workers, queue_capacity)` creates a fixed native worker
set and a bounded FIFO job queue. Both arguments are positive `u64` values.
Construction returns `Result[ThreadPoolExecutor, PoolError]` and starts all
workers before succeeding. Partial startup failure stops and joins the workers
already started. These names are explicit prelude builtins, like `spawn` and
`channel`; no syntax or implicit trait import is involved.

| Operation | Result | Behavior |
| --- | --- | --- |
| `pool.submit(job)` | `Result[Future[T], SubmitError[F]]` | Move an owned zero-argument job; wait for queue space |
| `pool.submit(function, arguments...)` | Same, with a compiler-bound job type | Bind an ordinary named function's owned arguments |
| `pool.submit_nowait(...)` | Same | Return `Full(job)` if the queue is full |
| `future.result()` | `Result[T, FutureError]` | Consume the future and wait for its result |
| `future.done()` | `bool` | Observe whether completed or cancelled |
| `future.wait_timeout(timeout_ms)` | `bool` | Observe completion or cancellation within a `u64` millisecond budget, without consuming the future |
| `future.cancel()` | `bool` | Prevent a pending job from starting; cannot interrupt running work |
| `pool.map(function, inputs)` | `Result[list[T], PoolMapError]` | Run a named function over an owned list or integer range, preserving input order |
| `pool.map_result(function, inputs)` | `Result[list[T], ParallelError[E]]` | Collect successes or the earliest input's worker error |
| `pool.reduce_tree(function, inputs)` | `Result[Option[T], PoolMapError]` | Combine adjacent pairs with fixed grouping |
| `pool.shutdown()` | `()` | Stop accepting work, drain accepted jobs, join all workers |
| `pool.shutdown(True)` | `()` | Cancel jobs still queued and join running workers |

For running work, [cooperative cancellation](38-cooperative-cancellation.md)
lets the worker decide when to stop. A timeout does not cancel a job; the future
remains available for a later `result()` call.

Methods other than `result()` observe their receiver; a `mut` binding is not
needed. Executors and futures are affine managed values: assignment transfers
ownership, and neither copying nor equality is supported. Futures can be stored
in collections, classes, and enums. A future has one consumer, with no result
cloning, shared-handle API, async/await, or event-loop machinery.

## Jobs and results

Submission accepts concrete wholly owned reusable or one-shot closures. Reusable
environments are invoked once and then released. Named submissions bind arguments
in an inline one-shot environment using ordinary argument type inference. Their
failure value is a callable job too. Borrowed arguments/captures, generator frames,
and uncertified indirect callable effects are rejected. Refer to the
[worker eligibility contract](32-scoped-native-threads.md) for other restrictions.

Jobs and results may have different concrete types on the same pool; generated
C adapters preserve their type and ownership information. The runtime scheduler
does not need language-level dynamic dispatch. Worker bodies, transitive calls,
stored fields, returned callbacks, and destructors undergo the existing
closed-world effect checks. Executors and futures cannot enter worker code,
including through nested stored values or channels. This rules out self-join and
workers waiting on other futures in an exhausted pool. Communication through
channels can still deadlock if the application's protocol cannot make progress.

An application `Result` remains a separate layer. A worker returning
`Result[T, E]` produces `Future[Result[T, E]]`; the first `?` on `result()` handles
cancellation and the next handles the application error. A job that exhausts its
worker's stack is not a job failure: it ends the process with a
[stack overflow report](09-rust-runtime-packaging.md#stack-overflow).

## Ordered mapping

Map requires a statically named one-argument function; generic arguments can be
inferred from the input element type. It consumes lists, moving their elements;
integer ranges retain their normal copyable semantics. The worker takes its
argument by value and returns a non-unit, owned storable result.

The initial map is eager and returns a list. It reserves output capacity before
executing work and keeps at most `max_workers + queue_capacity` jobs outstanding.
It waits for results in input order, independent of completion order. Memory is
bounded by the result list plus this window of job cells. Each cell still has a
fallible allocation. It does not allocate one future for every input in advance.

On an infrastructure error, map stops submitting, waits for already accepted work,
and drops its partial results and remaining inputs. Previously executed effects
are not rolled back. A worker's application `Err` is an ordinary list element;
map does not erase it or select one error implicitly. Generator inputs, borrowed
inputs, multiple iterables, closure mapping, and streaming output are not part of
this initial API.

Use [map_result](37-parallel-operations.md) to extract successful values or return
the earliest input's concrete error with deterministic cleanup.

See [ownership, errors, and shutdown](36-thread-pool-executors/01-ownership-and-shutdown.md)
for precise failure and cleanup guarantees, and the
[runnable lesson](../tutorial/87-reuse-workers-with-an-executor.md) for examples.
