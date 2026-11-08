# Store stateful callbacks

Pair explicit state with a named function when a registry needs different
callback bodies with the same state type and call signature:

```plenty
class Handler[State]:
    state: State
    callback: Callable[[&mut State, i64], Result[i64, OverflowError[i64]]]
    def call(self: &mut Handler[State], value: i64) -> Result[i64, OverflowError[i64]]:
        self.callback(&mut self.state, value)

def accumulate(total: &mut i64, value: i64) -> Result[i64, OverflowError[i64]]:
    *total = *total + value
    Ok(*total)

def subtract(total: &mut i64, value: i64) -> Result[i64, OverflowError[i64]]:
    *total = *total - value
    Ok(*total)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut handlers = [Handler(10, accumulate), Handler(100, subtract)]?
    for handler in &mut handlers:
        print(handler.call(3)?)?
    print(handlers[0].call(2)?)?
    Ok(())
```

```output
13
97
15
```

Each record owns its state; `call` borrows it exclusively. Constructing the
records needs no allocation, but storing them in a list does.

For different state shapes, use an enum and handle its variants in the callback.
[Owned concrete closures](callback-records.md) can also be stored when their
producer types agree. To store closures that each keep different captured
state, box each one as a [`BoxedMutCallable`](boxed-callbacks.md).

See `examples/callback_registry.plenty` for a registry example.
