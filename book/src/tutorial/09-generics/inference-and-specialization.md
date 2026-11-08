# Infer or choose a specialization

Inference matches the signature against the argument types. In `first(&values)`,
the parameter is `&list[T]` and the argument is `&list[i64]`, so `T` is `i64`.
When multiple arguments determine the same parameter, their types must agree.

The expected result type is evidence too. This function has no input mentioning
`T`, so the annotation on the receiving binding chooses it:

```plenty
def empty[T]() -> Result[list[T], AllocError]:
    []

def main() -> Result[(), AllocError | IoError]:
    values: list[u8] = empty()?
    print(values)?
    Ok(())
```
```output
[]
```

The expectation can also come from a return type, a concrete parameter, or an
enclosing `Some` or `Ok`. Evidence from the arguments and the expected result
must agree; a disagreement is an error, not a choice between them. Without any
evidence, write the type arguments:

```plenty-error
def empty[T]() -> Result[list[T], AllocError]:
    []

def main() -> Result[(), AllocError | IoError]:
    values = empty()?
    Ok(())
```
```error
cannot infer type parameter `T` for `empty` from its arguments
```

Here `values = empty[u8]()?` works. An unsuffixed literal in a generic position
uses its normal default (`i64` or `f64`) unless the expected result determines
that position's type; another argument does not change it. Keep explicit type
arguments when you want them to guide literals: `sum_to[u16](5)` is equivalent
to `sum_to(5u16)`. See [type argument inference](../../design/language/07-generics/inference.md)
for the complete rules.

Calls with the same concrete types share one compiled specialization, including
recursive calls. Independent uses can specialize many different helpers, or
the same helper for many types. The compiler separately limits how deeply one
new specialization requires another, how many one generic function has, and how
many are created in total. These are compile-time resource limits; generic
calls add no runtime allocation.
See [specialization limits](../../design/toolchain/02-architecture/emission.md)
for the current bounds.
