# Give constructors enough type information

A binding annotation, parameter, or return signature supplies missing type
information. `Some(42)` infers `Option[i64]` from its payload. `Nothing` needs
an option type from context; `Ok` and `Err` each need the other variant's type:

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

Qualify a constructor, such as `Option[i64].Nothing`, to supply its type
explicitly. Otherwise, insufficient context is an error:

```plenty-error
def main() -> ():
    answer = Ok(42)
```

```error
cannot infer `Ok`
```
