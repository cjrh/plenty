# Store different captured callbacks

Concrete closures remember where they were defined. To put different closures
behind one signature, explicitly allocate a `BoxedCallable`. Its constructor
consumes the callback and returns `Result`, so `?` handles allocation failure.

```plenty
def choose(k: i64) -> Result[BoxedCallable[[i64], Result[i64, OverflowError[i64]]], AllocError]:
    if k > 0:
        add = def [k](value: i64) -> Result[i64, OverflowError[i64]]:
            Ok(k + value)
        return BoxedCallable(add)
    span = range(10, 13)
    multiply = def [span](value: i64) -> Result[i64, OverflowError[i64]]:
        Ok(span[1] * value)
    BoxedCallable(multiply)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    callbacks: dict[str, BoxedCallable[[i64], Result[i64, OverflowError[i64]]]] = {
        "add": choose(3)?,
        "multiply": choose(0)?,
    }?
    print(callbacks["add"](4)?)?
    print(callbacks["multiply"](4)?)?
    borrowed = &callbacks["add"]
    print(borrowed(5)?)?
    Ok(())
```

```output
7
44
8
```

The factory's branches capture different data but return the same boxed signature.
Calling borrows the stored owner, so repeated calls keep it registered and require
no extra allocation. Ordinary moves transfer ownership, and dropping or replacing
the owner drops its captures. Named functions can also use `BoxedCallable(name)`.

`BoxedCallable` accepts reusable callbacks with owned captures that only read
them. A closure that updates its captures needs `BoxedMutCallable`, whose call
borrows the box mutably. Each box keeps its own state:

```plenty
def counter(step: i64) -> Result[BoxedMutCallable[[], Result[i64, OverflowError[i64]]], AllocError]:
    mut total = 0
    tick = def [mut total, step]() -> Result[i64, OverflowError[i64]]:
        total = total + step
        Ok(total)
    BoxedMutCallable(tick)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut handlers = [counter(1)?, counter(10)?]?
    for handler in &mut handlers:
        print(handler()?)?
    for handler in &mut handlers:
        print(handler()?)?
    Ok(())
```

```output
1
10
2
20
```

Iterate with `&mut` so each call can update its box. A
[one-shot closure](consuming-captures.md) needs `BoxedOnceCallable`, whose call
consumes and frees the box; see the
[one-shot queue example](consuming-callback-apis.md#queue-one-shot-jobs).

Borrowed captures, closures containing generators, and callbacks returning
references stay concrete. No boxed callback can cross into a worker thread.
Boxing does not make a callback copyable or comparable; use names as registry
keys. The [callback type guide](../../design/language/06-functions/05-boxed-callables.md#choosing-a-callback-type)
compares every kind of callback.
