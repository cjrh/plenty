# Build text with recoverable allocation

Use `text.concat(other)` to combine two strings. To combine a list of strings,
use `separator.join(parts)`. Both return `Result[str, AllocError]`:

```plenty
def greeting(names: &list[str]) -> Result[str, AllocError]:
    joined = ", ".join(names)?
    "Hello, ".concat(joined)?.concat("!")

def main() -> Result[(), Failure]:
    names = ["Ada", "Bea"]?
    print(greeting(&names)?)?
    print(names)?
    print("-".join([]?)?)?
    Ok(())
```
```output
Hello, Ada, Bea!
["Ada", "Bea"]

```

The methods borrow their inputs.
Joining first calculates the final size and then allocates one output buffer;
it does not build a succession of intermediate strings. An empty list gives an
empty string, and a one-element list adds no separator. Empty pieces still count:
joining `["a", "", "b"]` with `"-"` gives `"a--b"`.

UTF-8 characters and embedded `\0` are preserved. These methods require strings
and a `list[str]`; joining a generator or another kind of iterable is not
supported yet.
