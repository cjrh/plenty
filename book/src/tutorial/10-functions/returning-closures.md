# Return a closure

Return an owned closure to keep its captured data after its creating function
returns. `Closure` describes the returned call signature:

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

`apply` needs an exclusive borrow because calling the counter changes its state.
For a read-only closure, use a shared parameter such as `&Closure[[i64], i64]`.

Different anonymous expressions have different concrete environment types, even
with identical signatures. A factory must return one concrete type on every path.
Borrowing closures cannot escape their creating function.

An owned closure can capture another closure or be wrapped in `Option` or
`Result`:

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

Captured resources drop in reverse capture order, including nested environments.

You can call a returned closure immediately, for example `counter(10)()` or
`make(3).unwrap()(4)`. Its environment remains alive through the complete expression
and is then dropped. `?` in a later argument also cleans up that temporary.
