# Store owned closures

A generic record can own a callback's captured state. You do not need to name
the closure's concrete type: the constructor infers it.

```plenty
class Handler[F]:
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
