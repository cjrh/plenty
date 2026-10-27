# Source files and declarations

Spaces delimit indented suites. Tabs are rejected, and indentation must return
to an existing level. Blank lines and comment-only lines do not affect
indentation. Parentheses permit multiline expressions. Comments start with `#`.

Identifiers contain ASCII letters, digits, and underscores and cannot start with
a digit. Keywords and names beginning `__plenty_` are reserved. Diagnostics use
one-based line and column positions.

Plenty uses `def`, `True`, `False`, `and`, `or`, `not`, and `pass`. Strings accept
single, double, or triple quotes; triple quotes permit physical newlines. The
first standalone string in an indented function body is its documentation. Use
`return "text"` when that first string should be the returned value. A `b`
prefix makes a [byte literal](../03-types/bytes.md), `b"..."`, which holds ASCII
characters and `\xHH` escapes.

Module scope contains functions, classes, enums, protocols, type aliases,
[constants](modules.md#constants), static assertions, imports, and
[target conditionals](target-conditionals.md). Executable
statements and mutable bindings belong inside functions; there are
no global variables. Named functions belong at module scope; classes and
enums can declare methods, and protocols declare method requirements.
Anonymous functions can appear in function bodies; see
[functions](../06-functions/index.md).

Names follow [lexical scope and prelude rules](names.md). Modules use
[explicit imports](modules.md), with [visibility](visibility.md) controlled by
`pub`. An application has a [main entrypoint](entrypoint.md).
