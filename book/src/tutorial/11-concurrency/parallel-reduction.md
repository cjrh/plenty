# Combine values in parallel

`reduce_tree` combines adjacent pairs on the pool, then combines their results.
The function takes two values and returns either one value or a `Result` of that
value type. Give arithmetic callbacks their own fallible contract.

```plenty
def add[T: IntType](left: T, right: T) -> Result[T, OverflowError[T]]:
    Ok(left + right)

def main() -> Result[(), AllocError | IoError | PoolMapError | PoolError | OverflowError[i64]]:
    with ThreadPoolExecutor(2, 2)? as pool:
        print(pool.reduce_tree(add, range(1, 6))?)?
        print(pool.reduce_tree(add, range(0))?)?
    Ok(())
```

```output
Option[i64].Some(15)
Option[i64].Nothing
```

Empty input returns `Nothing`; otherwise the result is `Some(value)`. The outer
`Result` reports allocation failure, a closed pool, or the callback's precise
error. A fallible callback gives `Result[Option[T], PoolMapError | E]`. Lists
are consumed without copying their elements; ranges remain ordinary copyable
values.

The tree is fixed: `[a, b, c, d, e]` becomes `[f(a, b), f(c, d), e]`, then
`[f(f(a, b), f(c, d)), e]`, then one value. More workers do not change that
grouping. An odd final element advances unchanged.

Choose an operation whose grouping is suitable for your task. Subtraction and
floating-point addition can give different answers from a serial left-to-right
loop. Keep a serial loop when that exact order matters. Worker side effects
need not follow the tree order. For fallible callbacks, errors are selected
by tree level, then by left-to-right pair, not by completion time. Already
accepted jobs finish before the error returns; later levels do not run.
Fallible callbacks use one synchronization round per tree level rather than
the multi-level jobs available to infallible callbacks.

For larger inputs, reduction allocates scratch storage proportional to the input
length and only a bounded window of jobs. Empty and singleton reductions allocate
nothing. If allocation fails, the operation waits for accepted jobs and cleans
up every intermediate value before returning.
