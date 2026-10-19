# Return a reference to a field

Functions can return references when exactly one parameter is a reference. The
returned value must borrow that parameter, never a local owner. The caller keeps
the original value alive and borrowed until the result's last use.

```plenty
class Pair:
    left: i64
    right: i64
    def left_ref(self: &mut Pair) -> &mut i64:
        &mut self.left

def main() -> Result[(), IoError]:
    mut pair = Pair(1, 2)
    left = pair.left_ref()
    *left = 8
    print(pair.left)?
    Ok(())
```
```output
8
```

A mutable returned reference requires a mutable reference parameter. A temporary
receiver cannot supply a reference that outlives it.

Shared arguments can borrow temporaries, as in `size("ada")` for a `size(s: &str)`
function. The temporary lives through the expression. A result borrowing it may
be used immediately, but cannot be stored or returned. Mutable temporaries are
unsupported.

An exclusive method can use a completed read of its receiver as an argument.
This also works through an existing exclusive reference:

```plenty
import std.bytes
from std.bytes import Bytes, BytesError

def clear(buffer: &mut Bytes) -> Result[(), BytesError]:
    buffer.fill(0, buffer.len(), 0u8)

def main() -> Result[(), AllocError | IoError | BytesError]:
    mut buffer = std.bytes.from_list([1u8, 2u8]?)?
    clear(&mut buffer)?
    print(buffer.to_list()?)?
    Ok(())
```
```output
[0, 0]
```

The receiver is reserved first, arguments run left to right, and exclusive
access begins when the method starts. Arguments may read the receiver, but
cannot mutate or move it, or pass a reference into it that remains live during
the call. For nested mutations, compute the inner result in a separate local
before calling the outer method.
