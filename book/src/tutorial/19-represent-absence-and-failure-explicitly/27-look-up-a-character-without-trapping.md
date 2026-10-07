# Look up a character without trapping

Use `text.get(index)` when the index might be out of range or character
allocation might fail. It returns `Result[Option[str], AllocError]`: `Result`
reports allocation failure, while `Option` tells you whether the index exists.

```plenty
def show_character(text: &str, index: i64) -> Result[(), Failure]:
    character = text.get(index)?
    match character:
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
and indices outside either end return `Ok(Nothing)` without allocating. A valid
index allocates one independent string for the character. Neither wrapper
allocates, and the original string remains available on every outcome.

In the example, `?` handles the allocation-error path and leaves an `Option[str]`
for the match. `Nothing` is a successful lookup with no character, so it does not
propagate an error. Ordinary `text[index]` still terminates the program for a
missing index; allocation failure is returned in its Result. Both forms scan UTF-8 to reach the requested
scalar; repeated indexing is not a constant-time way to traverse text.
