# Replace duplicated functions with a type parameter

Suppose the same operation is needed for two integer widths. Separate
functions work, but repeat both the implementation and its interface:

```plenty
def keep_small(value: u8) -> u8:
    value

def keep_large(value: i64) -> i64:
    value

def main() -> Result[(), Failure]:
    print(keep_small(7u8))?
    print(keep_large(9))?
    Ok(())
```
```output
7
9
```

A type parameter lets one declaration express that relationship: the result
has the same concrete type as the argument. The compiler checks each concrete
specialization; this does not introduce a runtime type conversion.

Declare type parameters after the function name. Calls infer them from the
argument types, or you can supply them explicitly before the call.
Ownership still follows the concrete type: `identity(values)` transfers the
list, while a function taking `&T` borrows its argument.

```plenty
def identity[T](value: T) -> T:
    value

def sum_to[T: IntType](stop: T) -> Result[T, AllocError]:
    mut total: T = 0
    for n in range[T](stop):
        total = total + n
    Ok(total)

def first[T](values: &list[T]) -> &T:
    &values[0]

def main() -> Result[(), Failure]:
    print(sum_to(5u16)?)?
    values = identity([3, 4]?)
    print(first(&values))?
    print(values)?
    Ok(())
```
```output
10
3
[3, 4]
```

`IntType` restricts a parameter to integer types. Unconstrained `T` is useful
when the body only moves, borrows, or uses operations supported by the chosen
concrete type. Each specialization is checked and compiled once, whether the call
uses explicit or inferred type arguments. The following lessons apply the same idea to class fields, enum payloads,
and methods.
