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

There are two fallible operations: building text and printing it. This example
uses `Failure` to propagate either cause when detailed recovery is unnecessary.

Use `str.from(value)` to convert a scalar to text, or `str.repr(value)`
for a representation such as quoted text or a displayed result wrapper.
Both return allocation results; the [text conversion lessons](../08-resources/index.md)
show how to use them in a useful program.

A helper can compose these operations:

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

Allocating operations expose failures even when particular small values fit
inline. Collection displays, collection constructors, copying, and collecting an iterator
follow this same result pattern.
