# Replace literal text with recoverable allocation

`text.replace(old, new)` replaces every non-overlapping match, scanning left
to right, and returns `Result[str, AllocError]`. It borrows all three strings:

```plenty
def rename(text: &str) -> Result[str, AllocError]:
    updated = text.replace("Plenty", "plenty")?
    Ok(updated)

def main() -> Result[(), IoError]:
    original = "Plenty is Plenty"
    print(rename(&original))?
    print(original)?
    print("aaaaa".replace("aa", "X"))?
    print("Aé".replace("", "-"))?
    print("banana".replace("na", ""))?
    Ok(())
```

```output
Result[str, AllocError].Ok("plenty is plenty")
Plenty is Plenty
Result[str, AllocError].Ok("XXa")
Result[str, AllocError].Ok("-A-é-")
Result[str, AllocError].Ok("ba")
```

An empty search string inserts the replacement before, between, and after Unicode
scalars. An empty replacement removes matches. Matches are literal and inserted
text is not searched again; regexes and a replacement-count limit are not supported.
The result owns its bytes and survives destruction of every input.

The runtime checks the complete output size and allocates only the final string.
Even a call with no matches, or an empty result, makes that allocation and can
return `OutOfMemory`; unrepresentable sizes return `CapacityOverflow`. Failure
leaves all inputs unchanged. Constructing the arguments follows their own
allocation policies.
