# Call C libraries

Declare C functions in a `.plentyi` interface and import it as a module. C's
`abs` uses `int`, which is `i32` on Plenty's supported target. The wrapper rejects
the minimum integer, which C cannot negate.

Save this as `c_math.plentyi`:

```plenty-file c_math.plentyi
extern def abs_raw(value: i32) -> i32 = "abs"

pub def absolute(value: i32) -> Option[i32]:
    if value == -2147483648i32:
        return Nothing
    Some(abs_raw(value))
```

```plenty
import c_math

def main() -> Result[(), IoError]:
    print(c_math.absolute(-42i32))?
    print(c_math.absolute(-2147483648i32))?
    Ok(())
```

```output
Option[i32].Some(42)
Option[i32].Nothing
```

The C standard library is already linked. For another library, pass its archive
or driver options explicitly, for example `--link-arg /path/to/libexample.a` or
`--link-arg -L/path/to/libs --link-arg -lexample`. A shared library also needs to
be discoverable by the system loader when the application runs.

The interface author must check the C header and document pointer lifetimes,
ownership, and errors. Plenty does not verify the native implementation;
application code should use typed wrappers.
`pub` exposes a name to Plenty imports; it does not export a C symbol. See the
[C interface reference](../../design/facilities/04-interop/trusted-declarations.md) for the supported ABI.

## Call C on worker threads

A [worker thread](../11-concurrency/index.md) cannot reach a foreign call
unless its declaration is annotated `@thread_safe`. Plenty cannot see what C
code does, so this annotation is your promise that the function is safe on any
thread, at the same time as other calls, and changes no state that other
threads share, such as the working directory. POSIX documents `getpid` as safe:

```plenty-file c_process.plentyi
@thread_safe
extern def getpid_raw() -> i32 = "getpid"

pub def id() -> i32:
    getpid_raw()
```

```plenty
import c_process

def same_process(parent: i32) -> bool:
    c_process.id() == parent

def main() -> Result[(), IoError | SpawnError[()]]:
    parent = c_process.id()
    with spawn(same_process, parent).drop_job()? as task:
        print(task.join())?
    Ok(())
```

```output
True
```

Without the annotation, checking rejects the program and names `getpid`. Leave
it off when the C documentation does not state thread safety. An opaque
pointer also needs a thread contract on its type before worker code may hold
it; [own a foreign handle](foreign-resources.md#move-the-owner-to-a-worker)
shows one. See
[worker threads](../../design/facilities/04-interop/trusted-declarations.md#worker-threads)
for the full rules and binding guidelines, and
[how std.dataframe wraps its engine](../../design/facilities/04-interop/dataframe-binding.md)
for a complete library binding that follows them.

## Interfaces driven by a host

A plugin host can retain its own buffers and call Plenty through C callbacks.
Such bindings need different lifetime and aliasing contracts from a call-scoped
borrow. The [raw host interface reference](../../design/facilities/04-interop/raw-host-interfaces.md)
describes the trusted primitives for this case; the
[host-thread contract](../../design/facilities/04-interop/host-threads.md) explains
checked concurrent entry. Keep raw operations inside a small binding module and
keep ordinary Plenty references exclusive where required.

## Hosts that unload plugins

For a native host that closes and later reloads a shared library, add
`--unloadable-library` to the `--shared-library` build on Linux GNU x86_64.
The compiler checks the supported runtime subset; ordinary allocations and
collections are allowed. Before closing, finish all calls, destroy all owned
handles, and stop callbacks. Host threads may remain alive. Trusted C bindings
must also finish their background work and remove registrations that refer to
the library. See the [unloading contract](../../design/facilities/04-interop/unloading.md)
for the supported operations and complete teardown obligations.
