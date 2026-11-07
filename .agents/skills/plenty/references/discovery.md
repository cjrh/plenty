# Discovering capabilities before building a workaround

Use this guide when porting an application, designing a package, or evaluating
feedback that a capability is missing. The [module catalogue](../../../../book/src/design/stdlib/index.md)
is the starting point; the [signature index](../../../../book/src/design/stdlib/signatures.md)
and linked module references own the current API contracts.

## Start from the task

| Task | Where to look and what to distinguish |
|---|---|
| Write TOML configuration | [Typed TOML codecs](../../../../book/src/design/stdlib/toml-codecs.md): use `@serial` classes/enums with `load[T]` and `dump`; hooks are shared with JSON. `encode`, `encode_text`, `write_value` write dynamic value trees; `Document` preserves layout. Read TOML's null/integer/root restrictions before choosing a schema. |
| Render HTML or text | [std.template](../../../../book/src/design/stdlib/template.md): `engine`, `Engine.add`, `Engine.render`; HTML escaping is the default; `escape_text` and quote-specific filters select per-placeholder contexts on raw values. `html=False` disables default escaping; explicit escape filters still apply. Read its supported syntax and escaping contexts before substituting for another engine. |
| Locate per-user state | [std.fs.dirs](../../../../book/src/design/stdlib/fs/dirs.md): `state_dir`; returns an optional native-byte path, creates nothing, and has target/environment limits. |
| Parse a submitted form | [std.http.body](../../../../book/src/design/stdlib/http-body.md): bounded `parse_form_text` and `read_form`. Parsing key/value pairs is distinct from deriving typed validation for a class. |
| Preserve a partial I/O failure | [std.io](../../../../book/src/design/stdlib/io.md): `Partial` records completed work and the cause; it can be a member of an error union. [Typed streams](../../../../book/src/design/stdlib/io-typed.md) preserve a caller-chosen cause type. |
| Stop a worker or bounded wait | [Cancellation](../../../../book/src/design/facilities/03-concurrency/cancellation.md), [std.sync](../../../../book/src/design/stdlib/sync.md) deadline waits (`recv_before`, `select_recv_before`) and [std.ready](../../../../book/src/design/stdlib/ready.md) handle readiness. Channel, selection and future `_timeout_ms` waits take no token. A token does not interrupt arbitrary blocking calls. |
| Call native code from a worker | [Worker effects](../../../../book/src/design/facilities/03-concurrency/effects.md): a reachable foreign call, including from a destructor, is rejected unless its `extern def` is [`@thread_safe`](../../../../book/src/design/facilities/04-interop/trusted-declarations.md#worker-threads); an opaque pointer needs [`@thread_movable` or `@thread_shareable`](../../../../book/src/design/facilities/04-interop/trusted-declarations.md#opaque-types-on-worker-threads) on its type. Standard helpers also undergo certification; they have no blanket exemption. |

## Verify the report

Record `plenty --version`, the command used, target/platform, dependency versions
and locked revisions, and the actual loaded skill's path and release. Compare
those with the feature's reference: installed release behavior and checkout
behavior can differ. A proposal describes a design, not a shipped capability.
Do not diagnose a historical agent's mistake from today's skill alone.

Search the catalogue by task, then the signature index by related terms, error
or return types. Check submodules and aliases: finding no `std.toml.dumps`
does not rule out `encode_text`; `std.fs.dirs` and `std.http.body` are separate
modules. Read the linked ownership, allocation, limits and worker contracts.

Reduce the claim to a complete program or small package. Run `plenty --check`
(or `plenty check`), then execute it when effects, laziness or cleanup matter.
Keep the source, full diagnostic/output and version with the report. If a test
was only read or type-checked, say so rather than claiming execution.

Classify the result: an existing API, a different idiom, a version/platform
limit, a documented restriction, or a reproducible defect. For example, a
`match` cannot occupy every nested expression position; a match binding or
`a if condition else b` may express the intended lazy choice. Neither that
restriction nor a differently named writer warrants declaring the whole task
unsupported. See [conditional values](../../../../book/src/design/language/04-expressions/branches.md).

## Preserve the information callers need

A class-valued error can join a union; it need not be reduced to `Failure`.
This deliberately failing operation stands in for a partial stream write:

```plenty
from std.io import Partial, StreamError

def write_step() -> Result[(), Partial]:
    Err(Partial(3, StreamError.WriteZero))

def operation() -> Result[(), Partial | IoError]:
    write_step()?
    Ok(())

def main() -> Result[(), IoError]:
    match operation():
        case Ok(_):
            print("complete")?
        case Err(error):
            match error:
                case progress: Partial:
                    print(progress.completed, progress.cause)?
                case other: IoError:
                    print(other)?
    Ok(())
```
```output
3 std.io.StreamError.WriteZero
```

The demonstration recovers for observation; a real caller normally propagates
this typed error or performs an intentional recovery. See [error handling](errors.md)
for context, defaults and consistent exit status.

## Cancellable delay and shutdown

Use a token wait rather than a long unconditional sleep when a stop request
must wake a worker. Put the cancel guard after the spawned task so cancellation
happens before scope exit joins it, including early exits:

```plenty
def wait_for_stop(stop: CancellationToken) -> ():
    while not stop.wait_timeout_ms(1000):
        # Periodic work belongs here, between stop checks.
        pass

def main() -> Result[(), AllocError | SpawnError[()] | IoError]:
    stop = CancellationToken()?
    with spawn(wait_for_stop, stop.share()).drop_job()?, stop.cancel_on_exit():
        pass
    print("stopped")?
    Ok(())
```
```output
stopped
```

Any other wait in a loop that must stop on request takes the token too. For
channels and futures, use the `std.sync` waits `recv_before`, `send_before`,
`select_recv_before` and `future_before`; for I/O handles, use `std.ready` and
the `_before` methods. They return `WaitError.Cancelled` after `cancel()`. Do
not pick `recv_timeout_ms`, `send_timeout_ms`, `select_recv_timeout_ms` or a
future's `wait_timeout_ms` for such a loop: these low-level waits take no token,
so a long budget delays shutdown by that long. The
[signal lesson](../../../../book/src/tutorial/11-concurrency/stop-on-signal.md#stop-a-long-wait-on-a-signal)
stops a long selection on `SIGTERM`. Never assume cancellation releases
an unbounded child wait or foreign call.

## A confirmed worker restriction

A minimal negative check distinguishes an uncertified foreign call from an API
search failure. The companion interface is a declaration only; no native
library is needed because the program is rejected during checking:

```plenty-file ffi_probe.plentyi
pub extern def foreign() -> i64 = "external"
```

```plenty-error
import ffi_probe

def worker() -> i64:
    ffi_probe.foreign()

def main() -> Result[(), SpawnError[()]]:
    with spawn(worker).drop_job()?:
        pass
    Ok(())
```
```error
C function `external` has no worker-thread effect contract
```

The [effect reference](../../../../book/src/design/facilities/03-concurrency/effects.md)
also covers transitive helpers and destruction. Prefer an existing certified
standard operation when it expresses the task; renaming a foreign call or
moving it into a helper does not certify it. Add
[`@thread_safe`](../../../../book/src/design/facilities/04-interop/trusted-declarations.md#worker-threads)
to the `extern def` only after checking that the C function is safe on any
thread; the compiler trusts the annotation. Worker code holds an opaque
pointer only if its `opaque` declaration is `@thread_movable` (crosses only as
a field of a moved owner that `copy(x)` rejects) or `@thread_shareable` (crosses in
any form; no declared call may write through it, so build values on a movable
type first); every `extern def` taking a marked type must be `@thread_safe`.
`std.dataframe` and `std.linalg` values work on workers.

For worker HTTP server routing policy, keep the typed `Router[R]` and handler.
`set_fallback(Some(route))` and `set_method_not_allowed(Some(route))` select
ordinary `R` values; `Nothing` restores the built-in response. A custom 405
handler adds `Allow` from `exchange.allowed_methods`. Enable
`TrailingSlash.Redirect` explicitly for method-preserving 308 redirects after
an exact miss; exact routes and exact wrong-method responses win. `find` stays
an exact matcher. See [HTTP routing](../../../../book/src/design/stdlib/http-server.md#routing).

For trusted C bindings, `&list[str] as c_strings` creates a read-only call-scoped
null-terminated string-pointer vector; it shares `c_string`'s `CStrError`
allocation/NUL contract. `&mut i32 as errno` is a Plenty-only output parameter,
omitted from C arguments, which captures thread-local C errno immediately after
native return and before cleanup. Interpret the snapshot only when the native
return contract says it is meaningful; conversion failure leaves it unchanged.
Neither adapter permits retaining temporary pointers or bypasses worker checks.
See [foreign adapters](../../../../book/src/design/facilities/04-interop/borrowing-text.md).
