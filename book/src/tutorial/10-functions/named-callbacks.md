# Pass functions as values

Use a function's name without parentheses to pass it as a value. A `Callable`
annotation specifies its parameter types and return type:

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

You can call a value selected from a collection directly:

```plenty
def increment(value: i64) -> i64:
    value + 1

def main() -> Result[(), Failure]:
    handlers = [increment]?
    index = 0
    print(handlers[index](41))?
    Ok(())
```
```output
42
```

A callable can also borrow an argument and return unit:

```plenty
def increment(value: &mut i64) -> ():
    *value = *value + 1

def main() -> Result[(), Failure]:
    update: Callable[[&mut i64], ()] = increment
    mut value = 41
    update(&mut value)
    print(value)?
    Ok(())
```
```output
42
```
