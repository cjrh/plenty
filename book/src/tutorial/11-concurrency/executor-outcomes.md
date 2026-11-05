# Handle executor outcomes

A job returning `Result` keeps its application errors separate from submission
failure or cancellation. Retrieve its result, then handle the application result:

```plenty
def divide(value: i64) -> Result[i64, str]:
    divisor = NonZero.try_from(value).map_error(def(error: RangeError) -> str: "zero divisor")?
    # 24 // a nonzero i64 cannot overflow: the dividend is not i64.MIN.
    unchecked:
        Ok(24 // divisor)

type PoolJobError = PoolMapError | FutureError | SubmitError[()] | PoolError

def main() -> Result[(), AllocError | IoError | PoolJobError]:
    with ThreadPoolExecutor(2, 2)? as pool:
        future = pool.submit(divide, 0).drop_job()?
        match future.result()?:
            case Ok(value):
                print(value)?
            case Err(message):
                print(message)?
        print(pool.map(divide, [2, 0, 4]?)?)?
    Ok(())
```

```output
zero divisor
[Result[i64, str].Ok(12), Result[i64, str].Err("zero divisor"), Result[i64, str].Ok(6)]
```

The example prints the application error to show it; a program that cannot
continue propagates it instead. To propagate both layers from a single future,
write `(future.result()?)?`. Mapping preserves each application outcome in
input order.
