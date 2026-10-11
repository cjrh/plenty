# Store different captured callbacks

Concrete closures remember where they were defined. To put different closures
behind one signature, explicitly allocate a `BoxedCallable`. Its constructor
consumes the callback and returns `Result`, so `?` handles allocation failure.

```plenty
def choose(k: i64) -> Result[BoxedCallable[[i64], i64], AllocError]:
    if k > 0:
        add = def [k](value: i64) -> i64:
            k + value
        return BoxedCallable(add)
    span = range(10, 13)
    multiply = def [span](value: i64) -> i64:
        span[1] * value
    BoxedCallable(multiply)

def main() -> Result[(), Failure]:
    callbacks: dict[str, BoxedCallable[[i64], i64]] = {
        "add": choose(3)?,
        "multiply": choose(0)?,
    }?
    print(callbacks["add"](4))?
    print(callbacks["multiply"](4))?
    borrowed = &callbacks["add"]
    print(borrowed(5))?
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

This type currently accepts reusable shared-call callbacks with owned captures.
Keep mutable, one-shot, borrowed, or generator-containing closures concrete.
Callbacks returning references and worker transfer are also unsupported. Boxing
does not make a callback copyable or comparable; use names as registry keys.
