# Borrowing a field through a getter

A simple getter that directly returns a field preserves the field's identity
for the borrow checker. After the call, unrelated fields remain available:

```plenty
class Pair:
    x: i64
    y: i64
    def x_ref(self: &mut Pair) -> &mut i64:
        &mut self.x

def main() -> Result[(), Failure]:
    mut pair = Pair(1, 2)
    x = pair.x_ref()
    pair.y = 7
    *x = 9
    print(pair)?
    Ok(())
```
```output
Pair(x=9, y=7)
```

Getters that choose between fields borrow the whole argument. Indexed references
also borrow the whole collection. The call still requires the access declared by
the receiver type.
