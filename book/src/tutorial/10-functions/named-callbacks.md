# Pass functions as values

Use a function's name without parentheses to pass it as a value. A `Callable`
annotation specifies its parameter types and return type. The value stores
only a code address. To accept a callback with captured state too, use a
[generic callback parameter](generic-callback-apis.md).

```plenty
def double(value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value * 2)

def apply(operation: Callable[[i64], Result[i64, OverflowError[i64]]], value: i64) -> Result[i64, OverflowError[i64]]:
    operation(value)

def main() -> Result[(), IoError | OverflowError[i64]]:
    operation = double
    print(operation(6)?)?
    print(apply(double, 9)?)?
    Ok(())
```
```output
12
18
```

The callback's own return type includes `OverflowError[i64]`; `apply`
forwards that public `Result` and `main` propagates it with `?`. A caller
does not change a callback's arithmetic contract. See
[checked arithmetic](../../design/language/03-types/numbers.md#checked-blocks).

Function values can also be returned and reassigned through `mut` bindings.
Copying a function value copies its code address; no allocation is needed.

You can call a value selected from a collection directly:

```plenty
def increment(value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value + 1)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    handlers = [increment]?
    index = 0
    print(handlers[index](41)?)?
    Ok(())
```
```output
42
```

A callable can also borrow an argument and return a fallible unit result:

```plenty
def increment(value: &mut i64) -> Result[(), OverflowError[i64]]:
    *value = *value + 1
    Ok(())

def main() -> Result[(), IoError | OverflowError[i64]]:
    update: Callable[[&mut i64], Result[(), OverflowError[i64]]] = increment
    mut value = 41
    update(&mut value)?
    print(value)?
    Ok(())
```
```output
42
```

Class constructors and payload-bearing enum variants can be passed the same
way. Use the constructor directly when a callback only wraps a value:

```plenty
class Reading:
    value: i64

def main() -> Result[(), AllocError | IoError]:
    readings: list[Reading] = iter(range(3)).map(Reading).collect()?
    wrap: Callable[[i64], Option[i64]] = Some
    print(readings, wrap(7))?
    Ok(())
```
```output
[Reading(value=0), Reading(value=1), Reading(value=2)] Option[i64].Some(7)
```

For a generic constructor, an annotation supplies the instance, as for `wrap`
above. Without that context, name it explicitly: `Option[i64].Some`.
Constructor values take every positional argument and retain any initializer
failure in their result. See [constructor rules](../../design/language/03-types/classes.md).
