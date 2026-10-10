# Export owned objects

C receives an exported class as an opaque handle. The generated header names
its destroy function and ownership rules.

```plenty-file counter.plenty
pub class Counter:
    value: i64
    def __del__(self) -> ():
        print(self.value).unwrap()

pub export def create(value: i64) -> Result[Counter, AllocError] = "counter_create":
    Ok(Counter(value))

pub export def read(owner: &Counter) -> i64 = "counter_read":
    owner.value

pub export def increment(owner: &mut Counter) -> () = "counter_increment":
    owner.value = owner.value + 1

pub export def finish(owner: Counter) -> () = "counter_finish":
    drop(owner)
```

```plenty
import counter

def main() -> Result[(), Failure]:
    mut owner = counter.create(42)?
    counter.increment(&mut owner)
    print(counter.read(&owner))?
    print("ready")?
    counter.finish(owner)
    Ok(())
```
```output
43
ready
43
```

Build `counter.plenty` with `--shared-library --library-name counter` to get a C
factory returning a status and writing a `counter_Counter *` output on success.
A class returned to C must be declared as `Result[Class, AllocError]`, because
allocating its handle can fail. The library allocates the handle before running
the function, so a failure has no other effects. Release the output exactly once
with `counter_Counter_destroy`.

`counter_read` borrows a `const counter_Counter *`; `counter_increment` borrows a
`counter_Counter *` exclusively. Neither transfers ownership.

`counter_finish` consumes its handle. A C caller must not use or destroy that
handle again; a Plenty caller gets a moved-value error if it tries to reuse
`owner`. A consuming function keeps this rule even if it returns an error.

Plenty consumers get automatic cleanup through the generated `counter.plentyi`.
Keep the originating library loaded for every owner's lifetime.
