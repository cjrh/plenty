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
