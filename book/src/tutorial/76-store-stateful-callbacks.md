# Store stateful callbacks

A callback registry can pair explicit state with a named function. An ordinary
generic class expresses that pair; no new callable type or implicit environment
allocation is needed.

```plenty
class Handler[State]:
    state: State
    callback: Callable[[&mut State, i64], i64]
    def call(self: &mut Handler[State], value: i64) -> i64:
        self.callback(&mut self.state, value)

def accumulate(total: &mut i64, value: i64) -> i64:
    *total = *total + value
    *total

def subtract(total: &mut i64, value: i64) -> i64:
    *total = *total - value
    *total

def main() -> Result[(), Failure]:
    mut handlers = [Handler(10, accumulate)?, Handler(100, subtract)?]?
    for handler in &mut handlers:
        print(handler.call(3))?
    print(handlers[0].call(2))?
    Ok(())
```

```output
13
97
15
```

Each record owns independent state. `call` borrows that state exclusively for the
callback and preserves it for later calls. Constructing the records and list is
fallible; selecting and invoking a callback does not allocate. The callback's
body retains its ordinary allocation contract.

`State` can be a class containing owned resources, which drop normally with the
handler. Different callbacks can coexist when their state and call signatures
agree. For a finite set of different state shapes, use an enum and handle its
variants in the named callback. Arbitrary captured closure environments still
cannot be stored in these collections; this explicit-state pattern does not erase
their distinct types.

The repository also contains `examples/callback_registry.plenty` as a standalone
program demonstrating this pattern.
