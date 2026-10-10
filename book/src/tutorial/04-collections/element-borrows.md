# Borrowing collection elements

Borrow an element when you want to inspect or change it without moving its owner:

```plenty
def main() -> Result[(), Failure]:
    mut counts = {"visits": 1}?
    value = &mut counts["visits"]
    *value = 2
    print(counts)?
    Ok(())
```
```output
{"visits": 2}
```

The loan ends after its last use. While an element reference is live, the
collection cannot grow, remove entries, or move. Different indices are treated
as potentially overlapping. Missing keys and out-of-range indices still trap.
