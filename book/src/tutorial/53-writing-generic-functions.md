# Writing generic functions

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
uses explicit or inferred type arguments. Later lessons cover
[generic classes](73-parameterize-a-class.md) and
[generic methods](74-write-a-generic-method.md).

Inference matches the signature against the argument types. In `first(&values)`,
the parameter is `&list[T]` and the argument is `&list[i64]`, so `T` is `i64`.
When multiple arguments determine the same parameter, their types must agree.
Unsuffixed literals in generic positions use their normal defaults (`i64` or
`f64`); another argument does not change them. Keep explicit type arguments when
you want them to guide literals: `sum_to[u16](5)` is equivalent to `sum_to(5u16)`.

Types must be determined from arguments, not the expected return type. For
example, this function needs `empty[u8]()` because it has no input mentioning `T`:

```plenty
def empty[T]() -> Result[list[T], AllocError]:
    []

def main() -> Result[(), Failure]:
    values = empty[u8]()?
    print(values)?
    Ok(())
```
```output
[]
```

An annotation on the receiving binding does not change that rule:

```plenty-error
def empty[T]() -> Result[list[T], AllocError]:
    []

def main() -> Result[(), Failure]:
    values: list[u8] = empty()?
    Ok(())
```
```error
cannot infer type parameter `T` for `empty` from its arguments
```
