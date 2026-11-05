# Borrow and combine context managers

To keep a manager after the block, borrow it explicitly. The original binding
is exclusively borrowed until exit completes.

```plenty
class Stage:
    marker: i64
    def __enter__(self: &mut Stage) -> i64:
        self.marker = 1
        self.marker
    def __exit__(self: &mut Stage) -> ():
        self.marker = 11

def main() -> Result[(), IoError]:
    mut stage = Stage(0)
    with &mut stage as n:
        print(n)?
    print(stage.marker)?
    Ok(())
```
```output
1
11
```

`Stage` records lifecycle markers, not an accumulating counter: enter assigns
`1` and exit assigns `11`, including on repeated uses. Cleanup performs no
fallible arithmetic.

Several managers can share one `with` statement. They enter from left to right
and exit from right to left, just like nested blocks. Later manager expressions
can use earlier `as` bindings.

```plenty
class Label:
    text: str
    def __enter__(self: &mut Label) -> str:
        drop(print(self.text))
        self.text
    def __exit__(self: &mut Label) -> ():
        drop(print(self.text))

def main() -> Result[(), IoError]:
    with Label("first") as first, Label("second") as second:
        print("body")?
    Ok(())
```
```output
first
second
body
second
first
```

Exit is infallible and cannot suppress errors. Check fallible writes or flushes
explicitly. Fatal traps do not run exits, and `yield` inside `with` is not yet
supported.

`__enter__` can return a reference to the manager, giving the body access to its
methods and fields while `with` keeps ownership. The reference cannot escape.

```plenty
class Counter:
    count: i64
    def __enter__(self: &mut Counter) -> &mut Counter:
        &mut self
    def __exit__(self: &mut Counter) -> ():
        drop(print(self.count))

def main() -> Result[(), IoError]:
    with Counter(1) as counter:
        counter.count = 7
    Ok(())
```
```output
7
```

The lifecycle trace uses `drop(print(...))` deliberately: cleanup cannot return
an error, and a failed diagnostic write must not interrupt cleanup. For output
that must succeed, provide an explicit fallible operation and propagate its
result before leaving the scope.
