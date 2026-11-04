# Fixed-tree reduction

`pool.reduce_tree(function, inputs) -> Result[Option[T], PoolMapError]` accepts
a statically named `(T, T) -> T` worker and an owned list or integer range.
A named `(T, T) -> Result[T, E]` worker instead gives
`Result[Option[T], PoolMapError | E]`, preserving the ordinary public callback
error (including managed, affine payloads).
Generic types are inferred and [worker eligibility](index.md) applies. Lists
are consumed on every outcome; ranges retain copyable semantics. Empty input
returns Nothing; otherwise the result is Some. No identity is inserted or copied.

Each round combines adjacent pairs in left/right order; an odd final element
advances unchanged. Five inputs use `f(f(f(a, b), f(c, d)), e)`. Worker count,
queue capacity, and completion order do not change the tree. A successful
nonempty input of length n invokes the worker exactly n - 1 times.

The rounds define the grouping, not the schedule. For infallible callbacks,
one job reduces an aligned
run of adjacent inputs, a power-of-two count chosen so there is about one run
per worker, through as many rounds as the run spans; jobs then combine the run
results pairwise. A job's calls are serial, and runs proceed independently, so
calls from different rounds can overlap in time.

This reassociates operations rather than performing a left fold. Floating-point
results can differ from serial accumulation; pure workers produce the same result
for the same input and target across pool sizes. Effect order is unspecified.
Workers must not depend on another pair being scheduled. Workers retain their
[own arithmetic contracts](../../language/03-types/numbers.md#function-arithmetic-contracts).
General integer addition uses a callback returning
`Result[T, OverflowError[T]]` with explicit `Ok(left + right)`. A caller's
`checked:` or `unchecked:` mode never changes the callback or API type.

Fallible callbacks use separate adjacent-pair rounds, not multi-level run jobs.
All accepted jobs drain before returning. Callback errors are observed in
level order, then left-to-right pair order, never completion order. No later
level runs after an error. An accepted callback error takes precedence over a
later submission failure; without such an error the precise allocation or
shutdown failure is returned. Unused errors and intermediate values drop once.
This adds per-level synchronization and potentially more jobs than the
infallible optimized path, without changing successful grouping or replaying
callbacks. Empty and singleton input never invoke the callback.
A [parallel pipeline's](../../language/09-iteration/06-pipelines.md#parallel-pipelines)
`reduce` groups by chunk instead, so its result can depend on the chunk count;
use `reduce_tree` when one grouping is needed for every pool. Unlike
`reduce_tree`, a pipeline can read a borrowed list: `iter(&xs).par(&pool)`
borrows `xs` and maps its items to owned values before `reduce`.

Empty and singleton reductions allocate nothing. Larger inputs need one fallible
scratch buffer proportional to input length, including for ranges, a bounded job
window, fallible job cells, and, when a job reduces more than one input, one
small buffer of per-job run descriptors and pair scratch. Pair arguments transfer without tuple allocation
or implicit copying.

Infrastructure failure stops submission, drains accepted jobs, and destroys
intermediate and unsubmitted values. A closed pool returns Shutdown even for empty
input. When the callback returns exactly T and T is itself a Result, those Results
remain ordinary elements: only a distinct `Result[T, E]` callback is fallible.
