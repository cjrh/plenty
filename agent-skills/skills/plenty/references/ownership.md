# Ownership, borrowing, and allocation

Every value has one owner, which cleans it up at the end of its scope. There
is no garbage collector and no hidden allocation.

## Moves and copies

Scalars, `str`, tuples of copyable values, and `@copy` classes copy on
assignment. Collections, boxes, and other class instances move: the source
name becomes unusable. `copy(x)?` makes an explicit, fallible deep copy.

```plenty
def main() -> Result[(), AllocError | IoError]:
    names = ["Ada", "Grace"]?
    backup = copy(names)?
    kept = names
    print(kept, backup)?
    label = "team"
    other = label
    print(label, other)?
    Ok(())
```
```output
["Ada", "Grace"] ["Ada", "Grace"]
team team
```

## Borrowing in signatures

The parameter type states the access. `&T` borrows for reading, and the
caller may pass the owner directly. `&mut T` borrows exclusively, and the
caller must write `&mut`. A bare `T` takes ownership.

```plenty
def longest(words: &list[str]) -> i64:
    mut best = 0
    for word in words:
        if len(*word) > best:
            best = len(*word)
    best

def shout(words: &mut list[str]) -> Result[(), AllocError]:
    for word in words:
        *word = word.upper()?
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    mut words = ["plenty", "of", "room"]?
    print(longest(words))?
    shout(&mut words)?
    print(words)?
    Ok(())
```
```output
6
["PLENTY", "OF", "ROOM"]
```

Iterating `&list[T]` or `&mut list[T]` yields `&T` or `&mut T`; read and
replace through `*`. `for key, value in d.items():` borrows each value;
read it with `*value`.

Rules:

- A borrow lasts until its last use. Mutating the owner while a later use of a
  borrow remains is a `conflicting borrow` error.
- References cannot be stored in classes, enums, tuples, or collections.
- A function returning `&T` borrows the result from its first reference
  parameter; it cannot return a reference to a local or an owned parameter.
  When in doubt, return an owned value.
- Prefer `&str` and `&list[T]` parameters unless the function keeps the value.

## Classes

```plenty
from std.fmt import FormatError

class Account:
    owner: str
    balance: i64

    def deposit(self: &mut Account, amount: i64) -> ():
        self.balance += amount

    def describe(self) -> Result[str, AllocError | FormatError]:
        Ok(f"{self.owner}: {self.balance}"?)

def main() -> Result[(), AllocError | IoError | FormatError]:
    mut account = Account("Ada", 10)
    account.deposit(5)
    print(account.describe()?)?
    Ok(())
```
```output
Ada: 15
```

The constructor is positional and generated from the fields. Bare `self`
borrows the receiver; `self: &mut Class` mutates it; `self: Class` consumes it.

## Allocation is explicit

Anything that may allocate returns `Result[_, AllocError]`: displays,
comprehensions, `append`, `insert`, `add`, `reserve`, `copy`, string building
(`+`, `join`, `split`, `strip`, `upper`), and f-strings
(`Result[str, FormatError]`). Reserve when the size is known:

```plenty
def squares(count: i64) -> Result[list[i64], AllocError]:
    mut values = list[i64].with_capacity(count)?
    for n in range(count):
        values.append(n * n)?
    Ok(values)

def main() -> Result[(), AllocError | IoError]:
    print(squares(4)?)?
    Ok(())
```
```output
[0, 1, 4, 9]
```

Empty collections need a type: `xs: list[i64] = []?` or `list[i64]()?`.
Indexing a missing element or key traps; `xs.get(i)` and `d.get(k)` return
`Option`.

## Files are owned resources

`with open(path)? as file:` closes the file when the block ends, even on
error. Whole-file helpers `read_text(path)` and `write_text(path, text)` open
and close for you.

```plenty
def count_lines(path: &str) -> Result[i64, IoError]:
    mut count = 0
    with open(path)? as file:
        while True:
            line = file.readline()?
            if line == "":
                break
            count += 1
    Ok(count)

def main() -> Result[(), IoError]:
    write_text("lines.txt", "one\ntwo\n")?
    print(count_lines("lines.txt")?)?
    Ok(())
```
```output
2
```

`readline()` keeps the newline and returns `""` at end of file. A file is not
iterable with `for`.
