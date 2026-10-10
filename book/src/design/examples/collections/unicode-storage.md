# Text and Unicode values in a collection

Strings store explicit lengths, so an embedded NUL does not end the text.
Indexing returns one Unicode scalar as a `str`; negative indices count from
the end. Each scalar fits inline in the string value, so indexing does not
allocate. General text can share immutable storage.

```plenty
def main() -> Result[(), Failure]:
    text = "é\0😀"
    print(len(text))?
    print(text[-1])?
    print("\0" in text)?
    print([text]?)?
    Ok(())
```

```output
3
😀
True
["é\0😀"]
```
