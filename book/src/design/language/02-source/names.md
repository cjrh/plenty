# Names and scopes

Module declarations are visible throughout their module. Functions can call
later functions or recurse mutually; aliases and data declarations can refer
forward. Duplicate declarations and redefinitions are rejected.

Function parameters and local bindings occupy lexical scopes. A declaration in
a branch does not escape that branch: using the name afterwards is an unknown
binding, and the diagnostic says that the earlier binding ended with its block.
Bind a `mut` local to a value before the block and assign it inside instead. Assigning an existing mutable local changes
that local across a branch join. Pattern bindings belong to their arm and may
shadow outer bindings. A new binding cannot read itself in its initializer.

Type lookup is separate from local expression names. A local can shadow a numeric
cast name without changing type annotations. A binding shadowing an enum type
qualifier is diagnosed rather than selecting a different constructor.

The built-in prelude is available without an import. It provides primitive and
container type names (including `Box` and `Shared`), standard sums and
variants (`Option`, `Result`, `Some`, `Nothing`, `Ok`, `Err`), callable and
generator types, and facility names for
files, errors, threads, channels, executors, and cancellation. `FormatError`,
the error of user formatting methods and of f-strings that run them, names
`std.fmt.FormatError`; a module that
declares or imports its own `FormatError`, such as `std.report`'s, uses that
one instead. Built-in operations
include `copy`, `drop`, `len`, `range`, text I/O, worker submission, and channel
construction and selection. Their contracts appear under
[facilities](../../facilities/index.md) and the relevant language section.
Top-level declarations cannot redefine built-ins. Locals can shadow expression
names; unqualified standard variant patterns still denote the standard variants.

Imported names are explicit private bindings. Importing a module or protocol
does not activate methods or introduce transitive names.

An import binds its alias, or else its item or module path. That name cannot be
a built-in, a module declaration, or a name an earlier import bound. Importing
a standard declaration that is itself the built-in, such as
`from std.concurrent import ThreadPoolExecutor`, is allowed. The diagnostic
points at the bound name, says whether it conflicts with a prelude built-in or
with a declaration or import on a given line, and suggests renaming, binding
with `as`, or removing a redundant import. Function-local bindings may still
shadow imported names, as `math = 3` inside a function can after
`import std.math as math`.
