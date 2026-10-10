# Names and scopes

Module declarations are visible throughout their module. Functions can call
later functions or recurse mutually; aliases and data declarations can refer
forward. Duplicate declarations and redefinitions are rejected.

Function parameters and local bindings occupy lexical scopes. A declaration in
a branch does not escape that branch. Assigning an existing mutable local changes
that local across a branch join. Pattern bindings belong to their arm and may
shadow outer bindings. A new binding cannot read itself in its initializer.

Type lookup is separate from local expression names. A local can shadow a numeric
cast name without changing type annotations. A binding shadowing an enum type
qualifier is diagnosed rather than selecting a different constructor.

The built-in prelude is available without an import. It provides primitive and
container type names, standard sums and variants (`Option`, `Result`, `Some`,
`Nothing`, `Ok`, `Err`), callable and generator types, and facility names for
files, errors, threads, channels, executors, and cancellation. Built-in operations
include `copy`, `drop`, `len`, `range`, text I/O, worker submission, and channel
construction and selection. Their contracts appear under
[facilities](../../facilities/index.md) and the relevant language section.
Top-level declarations cannot redefine built-ins. Locals can shadow expression
names; unqualified standard variant patterns still denote the standard variants.

Imported names are explicit private bindings. Importing a module or protocol
does not activate methods or introduce transitive names.
