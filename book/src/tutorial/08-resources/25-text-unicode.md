# Build and normalize bounded text

`std.text` offers a reusable UTF-8 builder. Its limit counts bytes: `é` takes
two bytes and `🙂` takes four. A failed append leaves the old contents intact.

```plenty
import std.text
import std.unicode

def main() -> Result[(), AllocError | IoError | std.text.TextError]:
    mut message = std.text.builder(32)?
    message.append("Hello, ")?
    message.append("é🙂")?
    print(message.to_text()?, message.byte_len())?
    message.clear()
    message.append("again")?
    print(message.to_text()?)?
    words = ["one", "two", "three"]?
    print(std.text.join(&words, " / ", 32)?)?
    print(str.repr(std.text.wrap("é🙂abc", 2, 32)?)?)?
    print(std.unicode.version())?
    print(std.unicode.nfc("é", 16)? == "é")?
    print(std.unicode.nfkc("ﬃ", 16)?)?
    Ok(())
```
```output
Hello, é🙂 13
again
one / two / three
"é🙂\nab\nc"
17.0.0
True
ffi
```

`wrap` counts Unicode scalars, preserves LF, and can split a combining sequence;
it does not measure terminal columns or find word boundaries. Normalization
has a separate **scalar** limit covering the input and decomposed intermediate.
For example, decomposing `é` needs room for two scalars even though NFC can
compose them back to one. Every operation remains fallible: use `?` or match
`TextError` to handle allocation, invalid limits, and output that exceeds them.

Normalization never happens automatically. NFC treats canonically equivalent
spellings consistently; NFKC also removes compatibility distinctions such as
the ligature above. Choose it only when that loss is intended. Byte decoding
still rejects malformed UTF-8 before normalization; embedded NUL is ordinary
text. See [`std.text`](../../design/stdlib/text.md) and
[`std.unicode`](../../design/stdlib/unicode.md) for costs and complete contracts.
