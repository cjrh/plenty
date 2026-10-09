# Look up a character without trapping

Use `text.get(index)` when the index might be out of range. It returns
`Option[str]`: `Some(character)` when the index exists, `Nothing` otherwise.

```plenty
def show_character(text: &str, index: i64) -> Result[(), Failure]:
    match text.get(index):
        case Some(value):
            print(value)?
        case Nothing:
            print("missing")?
    Ok(())

def main() -> Result[(), IoError]:
    text = "café🙂"
    print(show_character(&text, -1))?
    print(show_character(&text, 3))?
    print(show_character(&text, 5))?
    print(text)?
    Ok(())
```

```output
🙂
Result[(), Failure].Ok(())
é
Result[(), Failure].Ok(())
missing
Result[(), Failure].Ok(())
café🙂
```

Indices are `i64` and count Unicode scalars, as with `text[index]`. Negative
indices count backward from the end; `-1` selects the last scalar. Empty strings
and indices outside either end return `Nothing`. Neither form allocates: a
character is a short string stored inline in the value, independent of the
original, which remains available.

Ordinary `text[index]` terminates the program for a missing index. Indexing
ASCII text takes constant time; other text is scanned from the start to reach
the requested scalar, so use `for character in text` to traverse it.
