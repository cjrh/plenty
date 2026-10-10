# Infer or choose a specialization

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
