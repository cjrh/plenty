# Build text with recoverable allocation

Concatenating strings can need new storage, so `+` returns
`Result[str, AllocError]`. Handle that result before using the new text:

```plenty
def main() -> Result[(), AllocError | IoError]:
    message = ('Hello, ' + "Plenty!")?
    print(message)?
    print(contains(message, "Plenty"))?
    Ok(())
```

```output
Hello, Plenty!
True
```

Building the string can fail with `AllocError`; printing can fail with
`IoError`. The `AllocError | IoError` sum lets this function propagate either
cause while keeping its original type and details.

Use `str.from(value)` to convert a scalar to text, or `str.repr(value)`
for a representation such as quoted text or a displayed result wrapper.
Both return allocation results; see the [text conversion lessons](../08-resources/index.md).

A helper can combine construction and printing behind one result:

```plenty
def greet(name: str) -> Result[(), AllocError | IoError]:
    print(("Hello, " + name)?)?
    Ok(())

def greet_if(enabled: bool, name: str) -> Result[(), AllocError | IoError]:
    if not enabled:
        return Ok(())
    greet(name)

def main() -> Result[(), AllocError | IoError]:
    greet_if(False, "Ada")?
    greet_if(True, "Ada")?
    Ok(())
```

```output
Hello, Ada
```

String-building operations return allocation results even when a particular
short string fits inline.
