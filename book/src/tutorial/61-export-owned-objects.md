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
```

```plenty
import counter

def main() -> Result[(), Failure]:
    owner = counter.create(42)?
    print("ready")?
    drop(owner)
    Ok(())
```
```output
ready
42
```

Build `counter.plenty` with `--shared-library --library-name counter` to get a C
factory returning a status and writing a `counter_Counter *` output on success.
Release that output exactly once with `counter_Counter_destroy`.

A separate Plenty consumer imports the generated `counter.plentyi` and links
the binary. Its owning wrapper cleans up automatically, just like the source
example. The wrapper needs one additional allocation; `create` reports that
failure through the same `AllocError` result. Keep the originating library loaded
for the lifetime of every owner.
