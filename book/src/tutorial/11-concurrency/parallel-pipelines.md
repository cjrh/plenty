# Run a pipeline on a pool

`.par(&pool)` runs the rest of a [pipeline](../05-iteration/pipelines.md) on a
pool's workers. Steps after it must be `map`, `filter` or `filter_value`, and the pipeline must
end in a terminal that combines the chunks: `collect`, `try_collect`, `count`,
`any`, `all`, `sum`, `min`, `max` or `reduce`.

```plenty
def score(n: i64) -> i64:
    n * n % 7

def positive(n: &i64) -> bool:
    *n > 0

def main() -> Result[(), AllocError | IoError | PoolMapError | PoolError]:
    with ThreadPoolExecutor(4, 4)? as pool:
        scores = iter(range(0, 10)).par(&pool).map(score).filter(positive).collect()?
        print(scores)?
        print(iter(range(0, 10)).par(&pool).map(score).sum()?)?
    Ok(())
```

```output
[1, 4, 2, 2, 4, 1, 1, 4]
19
```

The pool splits the input into one contiguous chunk per worker, and each
chunk runs the fused steps as one job. Results come back in input order. Every
terminal returns a `Result`, because submitting work can fail or the pool can
be shut down.

A list source works the same way. `iter(xs)` consumes the list; `iter(&xs)`
borrows it, yields `&i64` items as a serial pipeline does, and leaves `xs`
usable afterward. The chunks read the list in place instead of copying it.

```plenty
def big(n: &i64) -> bool:
    *n > 10

def main() -> Result[(), AllocError | IoError | PoolMapError | PoolError]:
    with ThreadPoolExecutor(4, 4)? as pool:
        xs = [3, 14, 15, 9, 26]?
        print(iter(&xs).par(&pool).filter(big).count()?)?
        print(iter(&xs).par(&pool).sum()?)?
        print(xs)?
    Ok(())
```

```output
3
67
[3, 14, 15, 9, 26]
```

Callbacks run concurrently, so they must be named functions or closures that
own their captures and change nothing. Put `enumerate` before `.par`:
the chunk offsets carry the index.

The chunking is fixed by the input length and the pool's worker count. A
floating-point `sum` adds each chunk, then adds the chunk totals, so it can
differ from a serial sum. `reduce` groups the same way: each chunk, then the
chunk results in order. Pass `chunks=N` to fix the count, for example
`.par(&pool, chunks=8)`, when the same answer is needed from pools of any
size. The [reference](../../design/language/09-iteration/06-pipelines.md#parallel-pipelines)
lists the rules and error types.
