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

def main() -> Result[(), Failure]:
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
