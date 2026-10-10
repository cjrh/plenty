# Keep callbacks in tuples and lists

A tuple can hold callbacks with different concrete types. Indexing invokes a
callback in place; unpacking transfers ownership to new local bindings.

```plenty
def add(offset: i64) -> Closure[[i64], i64]:
    def [offset](n: i64) -> i64:
        offset + n

def main() -> Result[(), Failure]:
    pair = (add(10), add(100))
    print(pair[0](2))?
    first, second = pair
    print(second(3))?
    Ok(())
```

```output
12
103
```

Calls to one factory also fit in a list. Each callback owns its own state.

```plenty
def counter(total: i64) -> Closure[[], i64]:
    def [mut total]() -> i64:
        total = total + 1
        total

def main() -> Result[(), Failure]:
    mut callbacks = [counter(n) for n in range(3)]?
    for callback in &mut callbacks:
        print(callback())?
    mut removed = callbacks.pop(0).unwrap()
    print(removed())?
    print(callbacks[0]())?
    Ok(())
```

```output
1
2
3
2
3
```

`pop` transfers the callback out of the list. A borrowed callback prevents
resizing or removing entries until its borrow ends.
