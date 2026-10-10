# Transfer a generator's progress

A generator owns a position in an advancing computation. Assignment transfers
that owner instead of copying the position:

```plenty
def once() -> Generator[i64]:
    yield 42

def main() -> Result[(), Failure]:
    first = once()
    second = first
    print(list(second)?)?
    Ok(())
```

```output
[42]
```

After a move, the old binding cannot be used:

```plenty-error
def once() -> Generator[i64]:
    yield 42

def main() -> ():
    first = once()
    second = first
    list(first).unwrap()
    pass
```

```error
use of moved binding `first`
```

The diagnostic starts with the file, line, and column of the use. A note
below it points at the move: `` `first` is moved here``.

Owned arguments, returns, and `for` iteration also move generators.
`next(&mut it)` borrows a mutable generator without consuming its owner;
`next(it)` remains a shorthand. A mutable binding can be reinitialized after
moving its previous value.

The compiler checks all possible continuing paths. A move on just one branch
makes later reuse unsafe, reported as `use of possibly moved binding`. Within a loop, an outer generator must be reinitialized
before any path repeats the loop. These rules are intentionally conservative.
Generators cannot be stored in collections or enum payloads yet.

Loans for local references and function arguments are checked before compilation,
as taught in [Borrow a collection](../04-collections/borrowing.md).

## Drop without resuming

Use `drop(value)` to release an owner earlier. The consumed binding becomes
unavailable; a mutable binding may then receive another value:

```plenty
def pending() -> Generator[i64]:
    print("started").unwrap()
    yield 1

def main() -> Result[(), Failure]:
    mut numbers = [1, 2, 3]?
    drop(numbers)
    numbers = [4]?
    print(numbers)?

    task = pending()
    drop(task)
    print("done")?
    Ok(())
```

```output
[4]
done
```

Dropping a generator cleans its captured values without resuming its body.
Borrow checking prevents dropping an owner while a reference still needs it.
A borrow can end at its last use; that does not implicitly destroy its owner.
