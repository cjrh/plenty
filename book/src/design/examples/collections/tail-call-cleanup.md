# Cleanup before a tail call

A function that ends with a call makes a tail call. Plenty
evaluates the arguments, cleans up the function's remaining values, and only then
enters the called function:

```plenty
class Resource:
    name: str

    def __del__(self) -> ():
        print(("release " + self.name).unwrap()).unwrap()

def finish(total: i64) -> i64:
    print("finish").unwrap()
    total

def step(total: i64) -> i64:
    scratch = Resource("scratch")
    finish(total + 1)

def main() -> Result[(), Failure]:
    print(step(41))?
    Ok(())
```

```output
release scratch
finish
42
```

`scratch` is released before `finish` runs. A value passed as an argument moves
to the called function, which cleans it up instead. Tail recursion therefore
keeps its constant stack use when each step owns a resource. A call that borrows
one of the function's values, such as `inspect(&scratch)` or a method call on it,
cannot release that value first: it runs as an ordinary call and cleanup follows.
A reference the function received as a parameter refers to a value outside the
function, so passing it on is still a tail call.
