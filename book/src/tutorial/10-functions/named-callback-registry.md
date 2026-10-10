# Name stored callbacks

A dictionary can associate names with owned callbacks from one factory. Use a
borrowed index call to keep the callback registered, or `pop` to take ownership.

```plenty
def counter(total: i64) -> Closure[[], i64]:
    def [mut total]() -> i64:
        total = total + 1
        total

def main() -> Result[(), Failure]:
    mut callbacks = {"open": counter(10), "close": counter(100)}?
    print(callbacks["open"]())?
    match callbacks.pop("close"):
        case Some(callback):
            mut owned = callback
            print(owned())?
        case Nothing:
            print("no close handler")?
    print(len(callbacks))?
    Ok(())
```

```output
11
101
1
```

Replacing a registered callback drops the old captured state. Inserting a new key
may grow the dictionary and returns `Result`. A live borrow of any callback
protects the dictionary against replacement, removal, and growth.

The callback itself cannot be a dictionary key or a set element: closures do not
define equality or hashing. The dictionary's value type preserves its concrete
factory identity, rather than accepting every callback with the same signature.
