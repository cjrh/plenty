# Trim surrounding whitespace

`strip()`, `lstrip()`, and `rstrip()` remove Unicode whitespace
from both ends, the left end, or the right end. Each returns `Result[str, AllocError]`:

```plenty
def main() -> Result[(), IoError]:
    text = "  é🙂  "
    print(text.strip())?
    print(text.lstrip())?
    print(text.rstrip())?
    print(" \t\n".strip())?
    Ok(())
```

```output
Result[str, AllocError].Ok("é🙂")
Result[str, AllocError].Ok("é🙂  ")
Result[str, AllocError].Ok("  é🙂")
Result[str, AllocError].Ok("")
```

They borrow the source and allocate only the final independent string. Empty
and unchanged results still need that allocation. Interior whitespace is kept;
NUL and zero-width space are not trimmed. These methods use Unicode White_Space
and take no arguments; custom character sets are not supported yet.
