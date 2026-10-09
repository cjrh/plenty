# Store owned closures

A generic record can own a callback's captured state. You do not need to name
the closure's concrete type: the constructor infers it.

```plenty
class Handler[F: Callable[[i64], i64]]:
    callback: F
    def call(self: &mut Handler[F], amount: i64) -> i64:
        self.callback(amount)

def counter(total: i64) -> Closure[[i64], i64]:
    def [mut total](amount: i64) -> i64:
        total = total + amount
        total

def main() -> Result[(), Failure]:
    mut first = Handler(counter(10))?
    mut second = Handler(counter(100))?
    print(first.call(3))?
    print(second.call(7))?
    print(first.callback(2))?
    Ok(())
```

```output
13
107
15
```

The `Callable` constraint checks the callback's signature when constructing the
record. It does not erase the concrete environment or allocate storage for it.
The records own independent environments. Each environment is stored directly
inside its record; the only construction allocation is the ordinary fallible
record allocation. Invoking these counters does not allocate. A callback may
still perform its own fallible work.

Keep captured state owned when storing a callback. A closure borrowing a local
cannot enter a record. Moving the record transfers ownership; dropping it cleans
up captured resources. Closures do not support `copy` or equality.

A tuple can hold callbacks with different concrete types. Indexing invokes a
callback in place; unpacking transfers ownership to new local bindings.

```plenty
def add(offset: i64) -> Closure[[i64], i64]:
    def [offset](n: i64) -> i64:
        offset + n

def main() -> Result[(), Failure]:
    pair = (add(10), add(100))?
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

`pop` returns `Option`: this example knows index zero exists. It transfers the
callback out of the list without allocating. A borrowed callback keeps the list
protected from resizing or removal until that borrow ends.
