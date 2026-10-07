# Export owned objects

A library factory can return a class without exposing its fields to C. Its
generated header names the matching destroy function and its ownership rules.

```plenty-file counter.plenty
pub class Counter:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()

pub export def create(value: i64) -> Result[Counter, AllocError] = "counter_create":
    Counter(value)

pub export def read(owner: &Counter) -> i64 = "counter_read":
    owner.value

pub export def increment(owner: &mut Counter) -> () = "counter_increment":
    owner.value = owner.value + 1
```

```plenty
import counter

def main() -> Result[(), Failure]:
    mut owner = counter.create(42)?
    counter.increment(&mut owner)
    print(counter.read(&owner))?
    print("ready")?
    drop(owner)
    Ok(())
```
```output
43
ready
43
```

Build `counter.plenty` with `--shared-library --library-name counter` to get a C
factory returning a status and writing a `counter_Counter *` output on success.
Release that output exactly once with `counter_Counter_destroy`.

`counter_read` borrows a `const counter_Counter *`; `counter_increment` borrows a
`counter_Counter *` exclusively. Neither transfers ownership. Plenty consumers
use `&owner` and `&mut owner` as in the example, with ordinary borrow checking.

A separate Plenty consumer imports the generated `counter.plentyi` and links
the binary. Its owning wrapper cleans up automatically, just like the source
example. The wrapper needs one additional allocation; `create` reports that
failure through the same `AllocError` result. Keep the originating library loaded
for the lifetime of every owner.
