# Common diagnostics

Each entry shows a Python habit, the compiler's message, and the fix. Messages
start with `error: FILE:LINE:COLUMN:`.

| Message (prefix) | Cause | Fix |
|---|---|---|
| `cannot move out of a field through a reference` | `for x in self.items` in a method | `for x in &self.items` |
| `executable statements are not allowed at module scope` | Code outside a function | Move it into `main` |
| `cannot implicitly discard a Result` | `print(x)` or another fallible call used as a statement | `print(x)?`, `match`, or `drop(result)` |
| `print cannot print a Result directly` | Printing an unhandled literal, `a + b`, or f-string | Add `?`: `xs = [1]?`, `(a + b)?`, `f"..."?` |
| `` `?` requires identical Result error types `` | The function's error type lacks the operand's error | Add it to the union: `Result[(), AllocError \| IoError]` |
| `` `?` operand and function return must use the same Result or Option family `` | `?` on an `Option` in a `Result` function | `.ok_or(error)?` |
| `` immutable binding: `x` is immutable `` | Reassigning a plain binding | Declare `mut x = ...` |
| `` unknown type `int` `` | Python type names | `i64`, `f64`, `bool`, `str` |
| `` unknown function `max` `` | `abs`, `min`, `max`, `sum` are not builtins | `from std.math import max` |
| `expected an expression` at `None` | No `None` | `Nothing`, with an `Option[T]` type |
| `expected an identifier` at `try` | No exceptions | Return `Result`; handle with `match` |
| `` `lambda` is not supported `` | Python lambdas | `def(x: i64) -> i64: x + 1` |
| ``expected `->` `` | Missing result type | Every `def` declares `-> T`; `-> ()` for none |
| `expected bool, got list[i64]` | Truthiness | `len(xs) > 0` |
| `expected i64, got &i64` | Using a borrowed loop item as a value | `*item` |
| ``expected `]` `` at `xs[1:3]` | Slice syntax | `xs.slice(1, 3)?` |
| `bare self is read-only in ordinary methods` | Mutating through `self` | `def bump(self: &mut C) -> ():` |
| `mutable reference arguments require explicit &mut borrowing` | Passing `xs` to a `&mut` parameter | `add(&mut xs)` |
| `` use of moved binding `xs` `` | Using a value after moving it | Borrow (`&xs`), or `copy(xs)?` |
| `conflicting borrow` | Mutating while a borrow is still used later | End the borrow before mutating |
| `mutable iteration requires a list` at `for line in file` | Files are not iterable | Loop on `file.readline()?` until `""`, or `read_text` and `split` |
| Runtime `dictionary key not found; use insert` | `d[k] = v` for a new key | `d.insert(k, v)?` |
| `expected Result[...], got ()` at the end of `main` | Missing final value | End with `Ok(())` |

<!-- Rows for `None`, `try`, `int`, `max`, `for line in file`, and the move-through-reference hint work around issues #570-#574 and #576; drop each row once its diagnostic explains the fix. -->

## Examples

```plenty-error
def main() -> Result[(), IoError]:
    xs = [1, 2]?
    print(xs)?
    Ok(())
```
```error
`?` requires identical Result error types
```

```plenty-error
def first(xs: &list[i64]) -> Result[i64, ParseError]:
    value = xs.get(0)?
    Ok(value)

def main() -> ():
    return
```
```error
`?` operand and function return must use the same Result or Option family
```

```plenty-error
def total(xs: &list[i64]) -> i64:
    mut sum = 0
    for x in xs:
        sum += x
    sum

def main() -> ():
    return
```
```error
expected i64, got &i64
```

```plenty-error
class Counter:
    hits: i64

    def bump(self) -> ():
        self.hits += 1

def main() -> ():
    return
```
```error
declare `self: &mut Counter` to mutate the receiver
```

```plenty-error
def add(xs: &mut list[i64]) -> Result[(), AllocError]:
    xs.append(1)?
    Ok(())

def main() -> Result[(), AllocError]:
    mut xs = [1]?
    add(xs)?
    Ok(())
```
```error
mutable reference arguments require explicit &mut borrowing
```

```plenty-error
def main() -> Result[(), AllocError | IoError]:
    numbers = [1, 2]?
    moved = numbers
    print(numbers)?
    Ok(())
```
```error
use of moved binding `numbers`
```

```plenty-error
def main() -> Result[(), AllocError | IoError]:
    mut numbers = [1, 2]?
    view = &numbers
    numbers.append(3)?
    print(view)?
    Ok(())
```
```error
conflicting borrow
```

## Matching unions

<!-- issue #575: drop this section once the diagnostic explains it -->
A variant pattern does not select a member of an error union:

```plenty-error
enum UsageError:
    Arguments

def run() -> Result[(), IoError | UsageError]:
    Err(UsageError.Arguments)

def main() -> i32:
    match run():
        case Ok(_):
            0i32
        case Err(UsageError.Arguments):
            2i32
        case Err(_):
            1i32
```
```error
expected IoError | UsageError, got UsageError
```

Bind the error and match its member by type:

```plenty
enum UsageError:
    Arguments

def run() -> Result[(), IoError | UsageError]:
    Err(UsageError.Arguments)

def main() -> Result[(), IoError]:
    match run():
        case Ok(_):
            print("ok")?
        case Err(error):
            match error:
                case usage: UsageError:
                    print("usage:", usage)?
                case io: IoError:
                    print("io:", io)?
    Ok(())
```
```output
usage: UsageError.Arguments
```
