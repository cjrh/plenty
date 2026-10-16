# Native effects on worker threads

Worker certification asks every native operation a body can reach for its
effect. The compiler classifies each primitive in one exhaustive table
(`src/effects.rs`), so a new primitive cannot be added without stating its
effect. Standard modules have no exemption: a `std` helper is certified by
the same walk as user code, and a foreign call is still rejected.

| Effect | Operations | On a worker |
|---|---|---|
| Compute | Text, collections, bytes, hashing, compression, Unicode | Allowed |
| Clock | Monotonic and wall clocks, `sleep` | Allowed |
| Entropy | `std.secrets` reads | Allowed |
| Process stream | Standard input, output and error, `print`, `args` | Allowed; no ordering between threads |
| Path | `open`, `read_text`, metadata, `read_dir`, `std.fs.root` | Allowed; a relative path resolves against the working directory at the call |
| Handle | Methods of `File`, `ReadDir` and `std.fs.Root`, rooted mutation, `std.ready` waits and selectors, operations on an open socket | Allowed under the loan rules below |
| Environment | `std.env.get`, `vars`, `current_dir` | Allowed; reads only |
| Child | `std.process` spawn, wait, poll and signal | Allowed; the runtime's child table is synchronized |
| Network | Opening a `std.net` socket, name lookup | Allowed; the kernel and C library synchronize sockets and the resolver |
| Process global | `std.env.set_current_dir` | Rejected |

Changing the working directory would change how every other thread resolves a
relative path, so a worker body, helper, callback or destructor that reaches
`set_current_dir` is rejected:

```text
error: worker `worker` reaches `std.env.set_current_dir`: changing the working directory affects every thread's relative paths; change it on the main thread before starting workers
```

Nothing in the language writes the environment, so there is no second
process-global effect today.

## Native handles

`File`, directory streams (`ReadDir`), entries and metadata cross thread
boundaries like any other owned value:

- An owned handle can move into a worker, a channel or an executor job. The
  receiving thread closes it, explicitly or when its owner drops.
- A scoped worker can borrow a handle. Reading, writing, seeking and closing
  need an exclusive loan, so the parent cannot use a handle a worker holds as
  `&mut File`, and two workers cannot hold it mutably at once.
- A shared loan (`&File`) permits only operations that do not move the cursor
  or change the handle: the `closed` property, `readable`, `writable`, rooted
  `openat` operations, and `std.ready` waits. Several workers may wait on one
  shared handle.
- A failed thread start drops each moved handle once, which closes it.

Foreign pointers (`opaque` types) remain ineligible: the compiler cannot know
whether a foreign resource tolerates use from another thread.

## Blocking and cancellation

Blocking calls stay ordinary function calls; there is no `async` syntax. A
worker that must not block indefinitely waits with a deadline through
[`std.ready`](../../stdlib/ready.md), which makes one bounded `ppoll` per step
on the handle and the [cancellation token](cancellation.md)'s wake
descriptor, so cancelling the token wakes the wait at once. A deadline that passes leaves the handle and any borrowed buffer
intact; the caller decides whether to retry, close or return. A token still
does not interrupt an unbounded `read`, `write`, `recv` or child `wait`.

## Resource use

Each worker is one native thread, started explicitly by `spawn`, an executor or
a parallel operation, so a program's thread count is what it asks for. No
facility starts a thread per handle or per connection; a
[`std.ready` selector](../../stdlib/ready.md#selectors) lets one worker wait on
many. An idle `std.ready` waiter does not wake until its handle is ready, its
deadline passes or its token is cancelled. `std.net.resolve` is the exception:
each lookup runs on a short-lived helper thread, at most 32 at once. Descriptors are
released when their owner closes or drops them, on whichever thread that is.

Supported target: x86_64 Linux GNU. Elsewhere `std.ready` waits report
`IoError.Unsupported`.
