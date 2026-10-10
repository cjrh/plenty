# Application exit and error reports

The native wrapper calls the application's `main` once. Unit and `Ok(())` produce
status zero, `i32` and `Ok(i32)` supply the status, and `Err` produces one. The
operating system may truncate statuses; small nonnegative values are portable.
Entrypoint signatures are defined in the
[language contract](../../language/02-source/entrypoint.md).

A returned `Err` produces one report, identically for compile-and-run and a
compiled executable. The wrapper flushes standard output, writes one line to
standard error, destroys the error payload once, and returns one. Intermediate
`?` propagation prints nothing.

The line begins `error: main returned ` followed by the payload rendered as
`str.repr` would render it. Errors needing an explanation add one after a colon:
`IoError.System(code)` uses the OS description, payload-free `IoError` variants
use fixed descriptions, and `Failure` states that it retains no original details.
Choose a concrete error type when the report should identify the cause.

Reporting is best effort. If allocation fails or the type cannot be rendered
(for example it contains a generator or recursive data), the wrapper writes the
allocation-free fallback `error: main returned Err`. Standard-error write failure,
including a closed descriptor or a pipe without a reader, is ignored. Every path
still destroys the payload and returns one.

Compilation/checking errors execute no program code. Runtime errors can leave
effects already performed. [Traps](../../language/08-errors/02-allocation-and-traps.md)
and [stack overflow](../02-architecture/runtime.md#stack-overflow) do not unwind.
