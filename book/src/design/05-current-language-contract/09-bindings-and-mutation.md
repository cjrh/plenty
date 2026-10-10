# Bindings and mutation

`name = expression` first declares an immutable local with inferred type.
`name: type = expression` provides an explicit type. `mut name = expression`
or `mut name: type = expression` declares a mutable local. Later `name = value`
assigns to an existing local and requires `mut` and the same type. Repeating
`mut` or a type annotation on an existing name is a duplicate declaration.
Parameters are immutable. Initializers cannot read their own new binding.

Indexed and field assignments accept nested writable paths such as
`rows[0][-1] = 9` and `records[0].values[1] = 7`. The root must be mutable or
an exclusive reference. Existing list slots and dictionary values update in
place, without allocating or copying their containing owners. Assignment does
not insert a missing dictionary key; use fallible `insert` for growth.

Evaluate the right-hand value first, then destination indices once each in
root-to-leaf order. Save their values before resolving any destination address;
index expressions may read or resize the destination. Bounds and key checks use
its resulting contents. Address resolution and the write hold an exclusive loan;
no user expression runs between them. Replacing an
owned value drops the previous value exactly once. A right-hand owner remains
tracked for cleanup if an index propagates an error with `?`.

Branch-local declarations do not escape their branch. Assignments to existing
mutable locals do persist across branch joins. There are no uninitialized
declarations. Parameters plus locals are currently limited to 256 slots per
function, a checked implementation limit inherited from the compact IR.

Binary applications require exactly one module-level `main` function with no
parameters and a return type of `()`, `i32`, `Result[(), E]`, or `Result[i32, E]`
(transparent aliases are accepted). The native wrapper calls it once: unit or
`Ok(())` maps to zero, an `i32` or `Ok(i32)` supplies the status, and `Err` maps to
one. The operating system may truncate that status; small nonnegative values are
portable.

An `Err` returned by `main` is reported once by the native wrapper, identically
for a compiled executable and for `plenty FILE`. The wrapper flushes standard
output, writes one line to standard error, drops the error payload exactly once
with its ordinary destructor, and returns status one. The line is
`error: main returned ` followed by the payload as `str.repr` renders it. The
standard errors whose rendering does not explain the failure get a description
after a colon: `IoError.System(code)` gets the operating system's text for the
code, each payload-free `IoError` variant gets a fixed sentence, and `Failure`
gets a statement that it keeps no details of the original error. Nothing is
printed at an intermediate `?`.

Reporting is best effort and cannot change the outcome. If the line cannot be
allocated, or the error type has no rendering (it contains a generator or
recursive data), the wrapper writes the fixed line `error: main returned Err`,
which needs no allocation. A failed write to standard error, including a closed
descriptor or a pipe without a reader, is ignored. In every case the payload is
still dropped and the status is still one. `main` follows ordinary function return typing,
borrow checking, and deterministic cleanup, including early/nonzero returns.
It cannot be a generator. Other functions, including forward declarations, are
ordinary callable functions and have no startup effects just by being declared.

Module scope accepts `def`, `class`, `enum`, and `type` declarations, plus imports.
Executable statements and bindings belong inside functions. `main` locals are
not globals. Missing or invalid entrypoints are diagnosed by both checking and
compilation before running code or creating an output artifact. Compilation and
type errors execute nothing. Runtime errors can leave effects that have already
occurred. Imported modules require no entrypoint, and their `main` declarations
are ordinary functions. Library checking is available; library object/shared
library output remains future work.
