# Handle executor outcomes

A job returning `Result` keeps its application errors separate from submission
failure or cancellation. Retrieve its result, then handle the application result:

```plenty
def divide(value: i64) -> Result[i64, str]:
    if value == 0:
        return Err("zero divisor")
    Ok(24 // value)

def main() -> Result[(), Failure]:
    with ThreadPoolExecutor(2, 2)? as pool:
        future = pool.submit(divide, 0)?
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
