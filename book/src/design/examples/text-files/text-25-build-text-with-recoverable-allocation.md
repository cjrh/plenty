# Build text with recoverable allocation

Use `text.concat(other)` to combine two strings. To combine a list or another
iterable of strings, use `separator.join(parts)`. Both return `Result[str, AllocError]`:

```plenty
def greeting(names: &list[str]) -> Result[str, AllocError]:
    joined = ", ".join(names)?
    "Hello, ".concat(joined)?.concat("!")

def main() -> Result[(), AllocError | IoError]:
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

UTF-8 characters and embedded `\0` are preserved. `join` also accepts a set,
a dictionary (its keys), a string (its scalars), or a generator of strings;
see [building text](../../facilities/01-builtins/text-building.md#joining) for
each form's allocations.
