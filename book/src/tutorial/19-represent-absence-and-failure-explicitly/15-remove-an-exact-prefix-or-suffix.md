# Remove an exact prefix or suffix

These methods remove one complete match at the chosen end. They return an
independent string inside a `Result`, leaving the original usable:

```plenty
def title(name: &str) -> Result[str, AllocError]:
    name.removeprefix("draft-")?.removesuffix(".txt")

def main() -> Result[(), IoError]:
    name = "draft-notes.txt"
    print(title(&name))?
    print("abab".removeprefix("ab"))?
    print("notes.txt".removesuffix(".csv"))?
    Ok(())
```

```output
Result[str, AllocError].Ok("notes")
Result[str, AllocError].Ok("ab")
Result[str, AllocError].Ok("notes.txt")
```

Empty patterns do nothing. Each method can fail to allocate even when the
contents stay unchanged. Use `strip` for surrounding whitespace instead.
