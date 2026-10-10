# Map fallible work in parallel

Use `map_result` when each worker returns a `Result` and you want either all the
successful values or one error. The error keeps its original type.

```plenty
def square(n: i64) -> Result[i64, str]:
    if n < 0:
        Err("negative input")
    else:
        Ok(n * n)

def main() -> Result[(), Failure]:
    with ThreadPoolExecutor(2, 2)? as pool:
        print(pool.map_result(square, range(5))?)?
        match pool.map_result(square, [3, -1, -2]?):
            case Ok(values):
                print(values)?
            case Err(error):
                match error:
                    case ParallelError[str].Worker(message):
                        print(message)?
                    case ParallelError[str].Allocation(error):
                        print(error)?
                    case ParallelError[str].Shutdown:
                        print("pool is closed")?
    Ok(())
```

```output
[0, 1, 4, 9, 16]
negative input
```

Outputs keep input order. If several inputs fail, the earliest input's error wins;
the fastest worker does not decide. `map_result` waits for accepted jobs and
cleans up partial results before returning. Later inputs may already have run, so
an error cannot undo their effects. Do not make a worker wait for an input that
might never be submitted.

Both `map` and `map_result` consume input lists and use a bounded job window.
Use ordinary `map` when you want a list containing every worker's `Result`,
including all errors. Both operations can report allocation failure.
