# 19. Represent absence and failure explicitly

`Option[T]` has two variants: `Some(T)` and `Nothing`.
`Some`, `Nothing`, `Ok`, and `Err` are built in and need no type prefix.
`Nothing` belongs to one concrete option type; it is not a universal null.

```plenty
def first_positive(values: list[i64]) -> Option[i64]:
    for value in values:
        if value > 0:
            return Some(value)
    Nothing

def main() -> Result[(), Failure]:
    for values in [[-1, 0]?, [-1, 42]?]?:
        match first_positive(values):
            case Some(value):
                print(value)?
            case Nothing:
                print("not found")?
    Ok(())
```

```output
not found
42
```

Use `Result[T, E]` when the absent result has an explanation. Its variants
are `Ok(T)` and `Err(E)`. The return signature supplies both types:

```plenty
def divide(left: i64, right: i64) -> Result[i64, str]:
    if right == 0:
        return Err("division by zero")
    Ok(left // right)

def main() -> Result[(), Failure]:
    for result in [divide(8, 2), divide(8, 0)]?:
        match result:
            case Ok(value):
                print(value)?
            case Err(message):
                print(message)?
    Ok(())
```

```output
4
division by zero
```

There is no implicit unwrapping or exception. Constructors get missing type
information from a binding annotation, function parameter, or return signature.
`Some(42)` already contains enough information to infer `Option[i64]`.
`Nothing` has no payload to infer from; `Ok` and `Err` each need the other
variant's type from context:

```plenty
def main() -> Result[(), IoError]:
    found = Some(42)
    missing: Option[i64] = Nothing
    success: Result[i64, str] = Ok(42)
    failure: Result[i64, str] = Err("not ready")
    print(found == Some(42))?
    print(missing == Option[i64].Nothing)?
    match failure:
        case Ok(value):
            print(value)?
        case Err(message):
            print(message)?
    Ok(())
```

```output
True
True
not ready
```

Qualified forms remain available when you want to state all types at the
construction site. A type alias works as well. Without sufficient context, the
compiler asks for a type instead of guessing:

```plenty-error
def main() -> ():
    answer = Ok(42)
```

```error
cannot infer `Ok`
```

For an operation that can fail but has no success data, return `Result[(), E]`
and construct success with `Ok(())`:

```plenty
def validate(name: str) -> Result[(), str]:
    if len(name) == 0:
        return Err("name is empty")
    Ok(())

def main() -> Result[(), Failure]:
    for result in [validate("Plenty"), validate("")]?:
        match result:
            case Ok(_):
                print("valid")?
            case Err(message):
                print(message)?
    Ok(())
```

```output
valid
name is empty
```

Unit is a real success payload, distinct from the absence represented by
`Nothing`. `Option[()]` also works. Standalone unit bindings and unit parameters
remain unsupported. User-defined enum variants still use their enum's prefix,
even if a variant happens to be named `Ok` or `Some`.
