# Slice text by Unicode scalar position

A [slice](../../language/04-expressions/slices.md) of a string returns
`Result[str, AllocError]`, with the same clamping rules as lists. Positions
count Unicode scalar values, just like `len` and indexing:

```plenty
def main() -> Result[(), AllocError | IoError]:
    text = "Aé🙂Z"
    print(str.repr(text[1:-1])?)?
    print(str.repr(text[-100:100])?)?
    print(str.repr(text[3:1])?)?
    print(str.repr(text[::-1])?)?
    print(str.repr(text.slice(1, -1))?)?
    print(text)?
    Ok(())
```
```output
Result[str, AllocError].Ok("é🙂")
Result[str, AllocError].Ok("Aé🙂Z")
Result[str, AllocError].Ok("")
Result[str, AllocError].Ok("Z🙂éA")
Result[str, AllocError].Ok("é🙂")
Aé🙂Z
```

`text.slice(start, stop)` is the method form, with required bounds and no step.
The result owns its UTF-8 bytes and outlives the source. The operation creates
only the final string, including for an empty or whole-string slice, so even
these cases can return `Err(AllocError.OutOfMemory)`. Source strings are never
modified. Combining marks count separately; positions are not grapheme clusters
or byte offsets.
