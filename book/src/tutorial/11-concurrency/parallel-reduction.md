# Combine values in parallel

`reduce_tree` combines adjacent pairs on the pool, then combines their results.
The function takes two values of the element type and returns one of that type.

```plenty
def add[T: IntType](left: T, right: T) -> T:
    left + right

def main() -> Result[(), Failure]:
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
`Result` reports allocation failure or a closed pool. Lists are consumed without
copying their elements; ranges remain ordinary copyable values.

The tree is fixed: `[a, b, c, d, e]` becomes `[f(a, b), f(c, d), e]`, then
`[f(f(a, b), f(c, d)), e]`, then one value. More workers do not change that
grouping. An odd final element advances unchanged.

Choose an operation whose grouping is suitable for your task. Subtraction and
floating-point addition can give different answers from a serial left-to-right
loop. Keep a serial loop when that exact order matters. The fixed tree stabilizes
pure computations; it does not order worker side effects.

For larger inputs, reduction allocates scratch storage proportional to the input
length and only a bounded window of jobs. Empty and singleton reductions allocate
nothing. If allocation fails, the operation waits for accepted jobs and cleans
up every intermediate value before returning.
