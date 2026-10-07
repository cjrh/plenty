# Bindings and mutation

`name = expression` first declares an immutable local with inferred type.
`name: type = expression` provides an explicit type. `mut name = expression`
or `mut name: type = expression` declares a mutable local. Later `name = value`
assigns to an existing local and requires `mut` and the same type. Repeating
`mut` or a type annotation on an existing name is a duplicate declaration.
Parameters are immutable. Initializers cannot read their own new binding.

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
