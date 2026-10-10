# Context managers

Files already showed why `with` is useful. To give your own class an action
at the end of a block, define the same entry and exit methods. Its `__enter__`
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
    with Message("hello") as text:
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
