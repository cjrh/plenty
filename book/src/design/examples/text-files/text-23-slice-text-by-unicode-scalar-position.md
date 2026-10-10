# Slice text by Unicode scalar position

Strings also have `slice(start, stop) -> Result[str, AllocError]`, using the
same required `i64` bounds and clamping rules as lists. Positions count Unicode
scalar values, just like `len` and indexing:

```plenty
def main() -> Result[(), Failure]:
    text = "Aé🙂Z"
    print(str.repr(text.slice(1, -1))?)?
    print(str.repr(text.slice(-100, 100))?)?
    print(str.repr(text.slice(3, 1))?)?
    print(text)?
    Ok(())
```
```output
Result[str, AllocError].Ok("é🙂")
Result[str, AllocError].Ok("Aé🙂Z")
Result[str, AllocError].Ok("")
Aé🙂Z
```

The result owns its UTF-8 bytes and outlives the source. The operation creates
only the final string, including for an empty or whole-string slice, so even
these cases can return `Err(AllocError.OutOfMemory)`. Source strings are never
modified. Combining marks count separately; positions are not grapheme clusters
or byte offsets.
