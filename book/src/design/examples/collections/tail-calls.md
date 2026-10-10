# Repeat work with tail recursion

A tail-recursive function can repeat work without growing the call stack.
Ordinary [range loops](../../../tutorial/02-values/range-loops.md) are usually
the first choice for repetition. This page explains recursion when a function
naturally calls itself.

```plenty
def sum_to(n: i64, total: i64) -> i64:
    if n <= 0:
        return total
    sum_to(n - 1, total + n)

def main() -> Result[(), IoError]:
    print(sum_to(100, 0))?
    Ok(())
```

```output
5050
```

The recursive call is the last operation on that path. Plenty reuses its call
frame in native code. An explicit
`return sum_to(n - 1, total + n)` works too. In contrast, `1 + recurse(...)`
still has addition to do after the call and is not a tail call.

Returning a range also works: the result's storage belongs to the original
caller and stays alive throughout the recursion.

```plenty
def countdown(n: i64) -> range[i64]:
    if n == 0:
        return range(2, 5)
    countdown(n - 1)

def main() -> Result[(), IoError]:
    print(countdown(1000))?
    Ok(())
```

```output
range(2, 5, 1)
```

Passing an owned range, record, or generator as an argument still needs an
ordinary call and consumes native stack space. A direct recursive tail call
with such an argument is rejected as described below. Returning one alone does
not impose that limit.

A direct recursive call in tail position must use a native tail transfer.
This also applies to mutual recursion: `first` calls `second`, which calls
back into `first`. Plenty reports an error when it cannot remove the caller's
frame safely. For example, the following call borrows a local that needs that
frame to stay alive:

```plenty-error
def walk(value: &i64, remaining: i64) -> i64:
    if remaining == 0:
        return *value
    child = remaining - 1
    walk(child, remaining - 1)

def main() -> ():
    pass
```

```error
recursive call to `walk` is in tail position but cannot be a tail call: the reference argument borrows local `child`
```

Pass a reference borrowed from the caller, carry owned scalar state, or use a
loop when the next step needs local storage. Non-tail recursion still uses the
native stack. Calls through function values are outside this direct-recursion
diagnostic; using a callback does not promise bounded stack use.
