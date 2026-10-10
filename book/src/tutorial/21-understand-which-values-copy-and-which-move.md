# 21. Understand which values copy and which move

Integers and booleans copy cheaply. Strings share immutable storage without
copying their bytes. Tuples and enums containing only these immutable values copy
too: their fields are stored inline, so a copy duplicates those bytes and shares
the strings. Collections, classes, generators, and tuples or enums containing
owned values move on assignment, owned argument passing, and return. Use
`copy(value)` to duplicate collections or their enclosing values. Nested mutable
contents are copied too; immutable strings can share storage. Generators and values containing
custom class cleanup cannot be copied.

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

Arguments, returns, and `for` iteration also move generators. `next` is the
exception: it temporarily uses a named mutable generator without consuming the
owner. A mutable binding can be reinitialized after moving its previous value.

The compiler checks all possible continuing paths. A move on just one branch
makes later reuse unsafe, reported as `use of possibly moved binding`. Within a loop, an outer generator must be reinitialized
before any path repeats the loop. These rules are intentionally conservative.
Generators cannot be stored in collections or enum payloads yet.

Loans for local references and function arguments are checked before compilation.
The next lesson explains how to use them.
