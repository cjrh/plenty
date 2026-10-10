# Fixed-tree reduction

`pool.reduce_tree(function, inputs) -> Result[Option[T], PoolMapError]` accepts
a statically named `(T, T) -> T` worker and an owned list or integer range.
Generic types are inferred and [worker eligibility](index.md) applies. Lists
are consumed on every outcome; ranges retain copyable semantics. Empty input
returns Nothing; otherwise the result is Some. No identity is inserted or copied.

Each round combines adjacent pairs in left/right order; an odd final element
advances unchanged. A round finishes before the next starts. Five inputs use
`f(f(f(a, b), f(c, d)), e)`. Worker count, queue capacity, and completion order
do not change the tree. A successful nonempty input of length n invokes the
worker exactly n - 1 times.

This reassociates operations rather than performing a left fold. Floating-point
results can differ from serial accumulation; pure workers produce the same result
for the same input and target across pool sizes. Effect order is unspecified.
Workers must not depend on another pair being scheduled. Integer overflow and
traps retain ordinary language behavior.

Empty and singleton reductions allocate nothing. Larger inputs need one fallible
scratch buffer proportional to input length, including for ranges, a bounded job
window, and fallible job cells. Pair arguments transfer without tuple allocation
or implicit copying.

Infrastructure failure stops submission, drains accepted jobs, and destroys
intermediate and unsubmitted values. A closed pool returns Shutdown even for empty
input. Worker Result values, when T is itself a Result, are ordinary elements;
reduction does not propagate or select an application error implicitly.
