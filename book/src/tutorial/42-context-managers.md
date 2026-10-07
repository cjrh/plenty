# Context managers

Use `with` when a class needs an action at the end of a block. Its `__enter__`
method supplies the `as` value; `__exit__` takes only a mutable receiver and
returns unit. No inheritance or protocol declaration is needed.

```plenty
class Message:
    text: str
    def __enter__(self: &mut Message) -> str:
        print("enter").unwrap()
        self.text
    def __exit__(self: &mut Message) -> ():
        print("exit").unwrap()

def main() -> Result[(), Failure]:
    with Message("hello")? as text:
        print(text)?
    print("after")?
    Ok(())
```
```output
enter
hello
exit
after
```

The manager moves into the block. The entry value and body locals are cleaned up
before `__exit__`, and the manager is dropped afterward. This also happens on
`return`, `?`, `break`, and `continue`; nested managers exit in reverse order.
An owned entry value can be moved out, for example by returning it. The `as`
binding itself is only visible inside the body. Omit `as` for unit entry methods.
Acquire fallible resources before entry: `with acquire()?:` propagates acquisition
failure without entering that context.

To keep a manager after the block, borrow it explicitly. The original binding
is exclusively borrowed until exit completes.

```plenty
class Counter:
    count: i64
    def __enter__(self: &mut Counter) -> i64:
        self.count = self.count + 1
        self.count
    def __exit__(self: &mut Counter) -> ():
        self.count = self.count + 10

def main() -> Result[(), Failure]:
    mut counter = Counter(0)?
    with &mut counter as n:
        print(n)?
    print(counter.count)?
    Ok(())
```
```output
1
11
```

Several managers can share one `with` statement. They enter from left to right
and exit from right to left, just like nested blocks. Later manager expressions
can use earlier `as` bindings.

```plenty
class Label:
    text: str
    def __enter__(self: &mut Label) -> str:
        print(self.text).unwrap()
        self.text
    def __exit__(self: &mut Label) -> ():
        print(self.text).unwrap()

def main() -> Result[(), Failure]:
    with Label("first")? as first, Label("second")? as second:
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

Most resource managers will return a reference to themselves. This gives the
body access to their methods and fields while `with` retains ownership and
arranges cleanup. The reference cannot escape the block.

```plenty
class Counter:
    count: i64
    def __enter__(self: &mut Counter) -> &mut Counter:
        &mut self
    def __exit__(self: &mut Counter) -> ():
        print(self.count).unwrap()

def main() -> Result[(), Failure]:
    with Counter(1)? as counter:
        counter.count = 7
    Ok(())
```
```output
7
```
