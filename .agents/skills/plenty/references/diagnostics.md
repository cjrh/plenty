# Common diagnostics

Each entry shows a Python habit, the compiler's message, and the fix. Messages
start with `error: FILE:LINE:COLUMN:`.

| Message (prefix) | Cause | Fix |
|---|---|---|
| `executable statements are not allowed at module scope` | Code outside a function | Move it into `main` |
| `cannot implicitly discard a Result` | `print(x)` or another fallible call used as a statement | `print(x)?`, `match`, or `drop(result)` |
| `print cannot print a Result directly` | Printing an unhandled literal, `a + b`, or f-string | Add `?`: `xs = [1]?`, `(a + b)?`, `f"..."?` |
| `` `?` requires identical Result error types `` | The function's error type lacks the operand's error | Add it to the union: `Result[(), AllocError \| IoError]` |
| `` `?` operand and function return must use the same Result or Option family `` | `?` on an `Option` in a `Result` function | `.ok_or(error)?` |
| `` immutable binding: `x` is immutable `` | Reassigning a plain binding | Declare `mut x = ...` |
| `` `lambda` is not supported `` | Python lambdas | `def(x: i64) -> i64: x + 1` |
| ``expected `->` `` | Missing result type | Every `def` declares `-> T`; `-> ()` for none |
| `expected bool, got list[i64]` | Truthiness | `len(xs) > 0` |
| `expected i64, got &i64` | Using a borrowed loop item as a value | `*item` |
| `` slice returns Result[list[i64], AllocError]; use `?` `` | A slice without `?` | `xs[1:3]?` |
| ``slice step must be `NonZero[i64]` `` | A computed `i64` step | `xs[::NonZero.try_from(step)?]?` |
| `bare self is read-only in ordinary methods` | Mutating through `self` | `def bump(self: &mut C) -> ():` |
| `mutable reference arguments require explicit &mut borrowing` | Passing `xs` to a `&mut` parameter | `add(&mut xs)` |
| `` use of moved binding `xs` `` | Using a value after moving it | Borrow (`&xs`), or `copy(xs)?` |
| `conflicting borrow` | Mutating while a borrow is still used later | End the borrow before mutating |
| `dictionaries have no indexed assignment` | `d[k] = v` or `d[k] += 1` | `d.insert(k, v)?`; change a value in place through `d.get_mut(k)` |
| `expected Result[...], got ()` at the end of `main` | Missing final value | End with `Ok(())` |

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
