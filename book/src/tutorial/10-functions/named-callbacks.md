# Pass functions as values

Use a function's name without parentheses to pass it as a value. A `Callable`
annotation specifies its parameter types and return type. The value stores
only a code address. To accept a callback with captured state too, use a
[generic callback parameter](generic-callback-apis.md).

```plenty
def double(value: i64) -> i64:
    value * 2

def apply(operation: Callable[[i64], i64], value: i64) -> i64:
    operation(value)

def main() -> Result[(), IoError]:
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

def main() -> Result[(), AllocError | IoError]:
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

def main() -> Result[(), IoError]:
    update: Callable[[&mut i64], ()] = increment
    mut value = 41
    update(&mut value)
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
