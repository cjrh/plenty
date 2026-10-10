# Build text with recoverable allocation

Concatenating strings can need new storage, so `+` returns
`Result[str, AllocError]`. Handle that result before using the new text:

```plenty
def main() -> Result[(), Failure]:
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
`IoError`. `Failure` lets this function propagate either cause.

Use `str.from(value)` to convert a scalar to text, or `str.repr(value)`
for a representation such as quoted text or a displayed result wrapper.
Both return allocation results; see the [text conversion lessons](../08-resources/index.md).

A helper can combine construction and printing behind one result:

```plenty
def greet(name: str) -> Result[(), Failure]:
    print(("Hello, " + name)?)?
    Ok(())

def greet_if(enabled: bool, name: str) -> Result[(), Failure]:
    if not enabled:
        return Ok(())
    greet(name)

def main() -> Result[(), Failure]:
    greet_if(False, "Ada")?
    greet_if(True, "Ada")?
    Ok(())
```

```output
Hello, Ada
```

String-building operations return allocation results even when a particular
short string fits inline.
