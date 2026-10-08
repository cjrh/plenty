# Return a closure

An owned environment can outlive the function that creates it. Describe its call
signature with `Closure`; the compiler retains the concrete environment layout.

```plenty
def counter(start: i64) -> Closure[[], i64]:
    def [mut start]() -> i64:
        start = start + 1
        start

def apply(f: &mut Closure[[], i64]) -> i64:
    f()

def main() -> Result[(), Failure]:
    mut first = counter(10)
    mut second = counter(30)
    print(apply(&mut first))?
    print(second())?
    print(first())?
    Ok(())
```
```output
11
31
12
```

Each call to `counter` creates independent inline state, without heap allocation.
`apply` borrows it exclusively because the closure changes that state. A read-only
closure can instead be passed to a parameter such as `&Closure[[i64], i64]`.

Different anonymous expressions have different concrete environment types, even
with identical signatures. A factory must return one concrete type on every path.
Borrowing closures cannot escape their creating function.

Owned closures can themselves be captured by another closure, and can be wrapped
in `Option` or `Result`. These wrappers and nested environments still need no heap
allocation:

```plenty
def make(offset: i64) -> Option[Closure[[i64], i64]]:
    inner = def [offset](value: i64) -> i64:
        offset + value
    outer = def [inner](value: i64) -> i64:
        inner(value) * 2
    Some(outer)

def main() -> Result[(), Failure]:
    calculate = make(3).unwrap()
    print(calculate(4))?
    Ok(())
```
```output
14
```

Moves preserve all nested state. When the final owner leaves scope, captured
resources are cleaned up exactly once, in reverse capture order.
