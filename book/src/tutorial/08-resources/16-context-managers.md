# Context managers

Define `__enter__` and `__exit__` to make a class usable with `with`. Entry
supplies the `as` value; exit takes only a mutable receiver and returns `()`.
No inheritance or protocol declaration is needed.

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

The manager moves into the block. Body locals and the entry value clean up before
`__exit__`; the manager drops afterward, including on early exits. An owned entry
value can move out, for example through `return`. The `as` binding is local to the
body; omit it for unit entry methods.

`with acquire()?:` propagates acquisition failure without entering the context.
