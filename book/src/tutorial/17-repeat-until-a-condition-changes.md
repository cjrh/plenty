# 17. Repeat until a condition changes

Use `while` when a condition determines how long to repeat. The condition must
be `bool`, and it is checked before every iteration, including the first:

```plenty
def main() -> Result[(), IoError]:
    mut remaining = 3
    while remaining > 0:
        print(remaining)?
        remaining = remaining - 1
    print("go")?
    Ok(())
```

```output
3
2
1
go
```

`continue` skips the rest of the current iteration. `break` exits the loop:

```plenty
def main() -> Result[(), IoError]:
    mut n = 0
    while True:
        n = n + 1
        if n % 2 == 0:
            continue
        if n > 5:
            break
        print(n)?
    Ok(())
```

```output
1
3
5
```

Update the condition's inputs before a `continue` when needed; otherwise a
`while` loop can repeat forever. In a `for` loop, `continue` automatically
advances to the next element:

```plenty
def main() -> Result[(), Failure]:
    for n in range(6):
        if n == 1:
            continue
        if n == 4:
            break
        print(n)?
    Ok(())
```

```output
0
2
3
```

Both statements affect only the innermost loop. Use `return` to leave a
function from inside any depth of loops. New bindings in a loop body stay
inside that body, while updates to enclosing `mut` bindings persist. Loops
have unit result; `break` cannot carry a value. Python's loop `else` clauses
are not supported.

The compiler conservatively assumes every loop can finish, even `while True`.
A function returning a value therefore still needs a result after the loop.
Statements directly after an unconditional exit are rejected:

```plenty-error
def main() -> ():
    while True:
        break
        print("unreachable").unwrap()
```

```error
unreachable statement after a control-flow exit
```
