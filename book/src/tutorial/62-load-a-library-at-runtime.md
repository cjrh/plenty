# Load a library at runtime

A program can choose a native library while it runs. Plenty still compiles the
function signatures ahead of time. Start with a small library, saved as
`counter.plenty`:

```plenty-file counter.plenty
class Counter:
    value: i64
    def __del__(self) -> ():
        print("released").unwrap()

export def create(value: i64) -> Result[Counter, AllocError] = "counter_create":
    Ok(Counter(value))

export def read(counter: &Counter) -> i64 = "counter_read":
    counter.value

export def increment(counter: &mut Counter) -> () = "counter_increment":
    counter.value = counter.value + 1
```

Build the shared library and its runtime-loading module:

```sh plenty-build
plenty --shared-library counter.plenty --library-name counter -o libcounter.so
plenty --runtime-interface counter.plenty --library-name counter -o plugin.plentyi
```

Now save this consumer beside `plugin.plentyi` and run it from the directory
containing `libcounter.so`. No library link arguments are needed.

```plenty
import plugin

def main() -> Result[(), Failure]:
    path = "./libcounter.so"
    library = plugin.load(&path)?
    mut counter = library.create(42)?
    library.increment(&mut counter)
    print(library.read(&counter))?
    drop(library)
    drop(counter)

    missing = "./missing-library.so"
    match plugin.load(&missing):
        case Ok(_):
            print("unexpected library")?
        case Err(error):
            print(error)?
    Ok(())
```
```output
43
released
LoadError.OpenFailed
```

`load` returns a `Result`, so `?` propagates loading failures. It checks the
library's exact interface metadata and resolves every required function before
returning. A changed contract produces `LoadError.IncompatibleContract`; a
missing export produces `LoadError.MissingSymbol`. Imports alone load nothing.

The `Library` caches function addresses. Its methods preserve the signatures,
borrows, and errors of the exports. The `counter` owns its native resource and
remembers the matching destructor, even after `library` is dropped. Both drops
above are optional early cleanup; scope exit does the same automatically.
Native code remains resident until process exit.

Keep objects with the library instance that created them. Passing an object to
a different loaded instance stops with a diagnostic before making the native
call. Loading trusts native code: metadata checks compatibility, and library
constructors may run before a later check reports an error.
