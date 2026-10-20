# Collection literals

Collection literals directly return `Result[collection, AllocError]`:

```plenty
def build() -> Result[list[list[i64]], AllocError]:
    [[1, 2]?, [3]?]

def main() -> Result[(), AllocError | IoError]:
    print({"answer": 42}?)?
    print(build()?)?
    Ok(())
```
```output
{"answer": 42}
[[1, 2], [3]]
```

Construction stops at its first allocation failure, drops the partial collection,
and skips later entries. The nested `?` operations extract each inner list and
propagate its failure to `build`.
Empty displays need context: `values: Result[list[i64], AllocError] = []`, or
`values: list[i64] = []?` inside a function returning `Result[..., AllocError]`.

## Byte keys and iterable conversion

Byte literals and tuples of hashable values can be dictionary keys or set
members. Call-style list constructors can collect text scalars or literal bytes:

```plenty
def main() -> Result[(), AllocError | IoError]:
    names = {b"GIF8": "gif"}?
    seen = {(b"tag", 1), (b"tag", 1)}?
    print(names[b"GIF8"], len(seen))?
    print(list("aé")?)?
    print(list[u8](b"AB")?)?
    Ok(())
```
```output
gif 1
["a", "é"]
[65, 66]
```

The `.from(...)` methods accept collections, ranges, and generators,
but not text or `bytes`. See [collection conversion](../../facilities/01-builtins/construction-copy.md#conversion)
for the supported constructor forms.
