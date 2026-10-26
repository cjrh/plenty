# Return a reference to a field

A function that returns a reference borrows it from its first reference
parameter; for a method, that is `self`. The returned value must borrow that
parameter, never a later one or a local owner. The caller keeps the original
value alive and borrowed until the result's last use.

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

A mutable returned reference requires the first reference parameter to be
`&mut`. A temporary receiver cannot supply a reference that outlives it.

Later reference parameters only lend their values for the call. A lookup can
take its key by reference and still return a borrow of the collection:

```plenty
def score(scores: &dict[str, i64], name: &str) -> &i64:
    &scores[*name]

def main() -> Result[(), AllocError | IoError]:
    scores = {"ada": 3, "bob": 5}?
    mut name = "bob"
    points = score(&scores, &name)
    name = "eve"
    print(*points)?
    print(name)?
    Ok(())
```
```output
5
eve
```

The key's loan ends when `score` returns, so `name` can change while `points`
is live. To return a borrow of `name`, it would have to be the first parameter.

## Return a borrow that may be absent

A search may find nothing. Return `Option[&T]`, or `Option[&mut T]` to let the
caller change what was found. `Some` must borrow the first reference parameter;
`Nothing` borrows nothing:

```plenty
class Item:
    name: str
    count: i64

def find(items: &mut list[Item], name: &str) -> Option[&mut Item]:
    for item in items:
        if item.name == name:
            return Some(item)
    Nothing

def add(items: &mut list[Item], name: str) -> Result[(), AllocError]:
    match find(items, &name):
        case Some(item):
            item.count += 1
        case Nothing:
            items.append(Item(name, 1))?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    mut items: list[Item] = []?
    add(&mut items, "ada")?
    add(&mut items, "bob")?
    add(&mut items, "ada")?
    for item in items:
        print(item.name)?
        print(item.count)?
    Ok(())
```
```output
ada
2
bob
1
```

Match the result directly. In the `Some` arm, `item` borrows `items`, so the
list cannot change there. The `Nothing` arm holds no borrow, so it can append.
`Result[&T, E]` works the same way with `Ok`; its error cannot hold a reference.
`?` and `unwrap()` also produce the reference.

The result can be held in a local until it is matched, but it cannot be stored
in a list, a field, or a `mut` binding: match it and store an owned copy or an
index. The [reference](../../design/language/05-ownership/05-optional-references.md)
lists every rule.

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
