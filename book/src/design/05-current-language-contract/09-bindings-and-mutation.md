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
root-to-leaf order. The destination is exclusively borrowed during index
evaluation; index expressions cannot access a conflicting loan. Replacing an
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
one. Errors are dropped normally; status conversion allocates no diagnostic.
Use explicit error handling to print an error message before returning. The operating system may truncate that status; small
nonnegative values are portable. `main` follows ordinary function return typing,
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
