# Return a reference to a field

A getter can let the caller borrow a field without moving the record.
The record remains responsible for its lifetime and cleanup.

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

This simple getter preserves the returned field's identity, so unrelated fields
remain available after the call. More complex getters conservatively protect the
whole argument. A mutable returned reference requires a mutable reference parameter.
Use a named receiver, class field, or element of named storage; temporary receivers
cannot supply a reference that outlives their owner.

Shared arguments can also be temporary values, such as `size("ada")` or
`size(&"ada")` for a `size(s: &str)` function. The temporary is evaluated once
and kept alive through the entire expression. A result borrowing that temporary
may be used immediately, but cannot be saved in a binding or returned. Mutable
temporaries are not supported.
