# Map fallible work in parallel

Use `map_result` when each worker returns a `Result` and you want either all the
successful values or one error. The error keeps its original type.

```plenty
def square(n: i64) -> Result[i64, str | OverflowError[i64]]:
    if n < 0:
        Err("negative input")
    else:
        Ok(n * n)

def main() -> Result[(), AllocError | IoError | ParallelError[str | OverflowError[i64]] | PoolError]:
    with ThreadPoolExecutor(2, 2)? as pool:
        print(pool.map_result(square, range(5))?)?
        match pool.map_result(square, [3, -1, -2]?):
            case Ok(values):
                print(values)?
            case Err(ParallelError.Worker(error)):
                match error:
                    case message: str:
                        print(message)?
                    case overflow: OverflowError[i64]:
                        return Err(ParallelError[str | OverflowError[i64]].Worker(overflow))
            case Err(error):
                return Err(error)
    Ok(())
```

```output
[0, 1, 4, 9, 16]
negative input
```

The fixed negative inputs deliberately demonstrate a handled worker error;
a program that cannot recover should propagate that error too. Allocation
failure, arithmetic overflow, and shutdown are not recovered here and leave `main` with an error.

Outputs keep input order. If several inputs fail, the earliest input's error wins;
the fastest worker does not decide. `map_result` waits for accepted jobs and
cleans up partial results before returning. Later inputs may already have run, so
an error cannot undo their effects. Do not make a worker wait for an input that
might never be submitted.

Both `map` and `map_result` consume input lists and use a bounded job window.
Use ordinary `map` when you want a list containing every worker's `Result`,
including all errors. Both operations can report allocation failure.

The worker preserves both its input error and arithmetic error in an
[anonymous sum](../03-absence/anonymous-sums.md). Matching the `str` member
prints its message rather than the structural representation of the sum.
