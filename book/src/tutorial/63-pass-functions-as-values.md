# Pass functions as values

Use a function's name without parentheses to pass the function itself. A
`Callable` annotation lists its parameter types and its return type:

```plenty
def double(value: i64) -> i64:
    value * 2

def apply(operation: Callable[[i64], i64], value: i64) -> i64:
    operation(value)

def main() -> Result[(), Failure]:
    operation = double
    print(operation(6))?
    print(apply(double, 9))?
    Ok(())
```
```output
12
18
```

Function values can also be returned and reassigned through `mut` bindings.
Copying a function value copies its code address; no allocation is needed.
