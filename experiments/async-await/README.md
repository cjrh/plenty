# Async/await design spike

An isolated source-to-C experiment supporting the
[proposal](../../book/src/proposals/async-await.md). This does **not** add async
syntax to Plenty. It uses Python's parser only; generated state machines execute
as native C, without Python's coroutine machinery, GC, or an async library.

Run from the repository root (Python 3, a C11 compiler, no third-party packages):

```sh
python3 -B experiments/async-await/test_spike.py
SANITIZE=1 python3 -B experiments/async-await/test_spike.py
python3 -B experiments/async-await/lower.py experiments/async-await/program.async
```

`CC` selects a compiler. The first command compiles a temporary native executable
and runs assertions; the second additionally enables address/undefined-behavior
sanitizers. Generated C is printed by the third. Tests leave no generated files
in the repository. Assertions must remain enabled in the C build.

## Language and runtime boundary

`program.async` uses `async def`, prefix `await`, positional `i64` parameters and
an `i64` result annotation. `x = await f(args)` embeds a concrete child frame;
`await tick(value)` suspends once then returns its value. Fresh bindings, integer
addition/subtraction and a final return are supported. Arithmetic wraps at
64 bits. `resource = acquire()` owns a 16-byte allocation; `fail(code)` terminates
with a numeric error. The harness, not source syntax, creates tasks/scopes.

Unlike the proposed Plenty spelling, spike results have an implicit error
channel and awaiting a failed child automatically propagates it. It does not
implement Plenty's `Result`, `?`, types, borrow checker, parser or code generator.
`acquire` and `tick` are experimental intrinsics, not proposed standard APIs.
The accepted source grammar is intentionally narrow, not a Python interpreter.

Borrowed parameters, recursion/forward calls, loops/branches, reassignment,
resource escape/copy, callbacks, `with`, `async with`, exception handling and
user-defined destructors are excluded. All functions return integers; no
reference can escape polling or be stored in a frame. The C harness is trusted
and explicitly invalidates moved-from owners; C itself does not enforce affine
ownership. `Scope` holds two task/result slots. Submission reserves capacity,
allocates one root frame and then moves it. Failure returns the source unchanged.
Direct nested await needs no allocation beyond the body. Closing the scope drops
remaining frames synchronously; this is valid for its memory-only resources.

There is no OS I/O, wake registration, timer, race, cancellation token, blocking
worker adapter, erased result allocation or cross-thread execution. `step` polls
each live root once in slot order. This is a deterministic test driver, **not** a
production executor or a fairness/performance experiment. Its fixed queue and
scalar outputs deliberately remove several allocation sites a real design must
account for.

See the [evidence page](../../book/src/proposals/async-await/evidence.md) for
measured layouts, assertions, limitations and resulting design conclusions.
