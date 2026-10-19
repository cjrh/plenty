# Handle executor outcomes

A job returning `Result` keeps its application errors separate from submission
failure or cancellation. Retrieve its result, then handle the application result:

```plenty
def submitted[T, F](result: Result[Future[T], SubmitError[F]]) -> Result[Future[T], str]:
    match result:
        case Ok(future):
            Ok(future)
        case Err(SubmitError.Full(_)):
            Err("submission queue is full")
        case Err(SubmitError.Shutdown(_)):
            Err("executor has shut down")
        case Err(SubmitError.OutOfMemory(_)):
            Err("submission allocation failed")
        case Err(SubmitError.CapacityOverflow(_)):
            Err("submission capacity overflow")

def divide(value: i64) -> Result[i64, str]:
    if value == 0:
        return Err("zero divisor")
    Ok(24 // value)

type PoolJobError = PoolMapError | FutureError | str | PoolError

def main() -> Result[(), AllocError | IoError | PoolJobError]:
    with ThreadPoolExecutor(2, 2)? as pool:
        future = submitted(pool.submit(divide, 0))?
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

Mapping preserves each application outcome in input order. To propagate both
layers from a single future, write `(future.result()?)?`.

Submission errors retain an unstarted job. Here `submitted` deliberately
releases that job and preserves the failure reason as a message. Match the
original `SubmitError` instead when you want to recover or retry the job.
