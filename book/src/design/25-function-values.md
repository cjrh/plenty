# Function values

A named function can be assigned, passed, and returned as a value. Its type is
`Callable[[parameter types], result type]`; parameter names are not part of type
identity. A no-argument function uses `Callable[[], T]`, and a function returning
unit uses `Callable[[T], ()]`. Aliases can name these signatures.

Function values are copyable code addresses. Creating, passing, returning, and
calling them needs no heap allocation. Calls use Plenty's internal ABI, including
caller-provided storage for inline results. They are not C function pointers.
Allocations performed by the called function retain their ordinary Result API.

The called function can be selected by an arbitrary expression: `choose()(n)`, `(f)(n)`,
`handlers[index](n)`, and `record.callback(n)` all work. A bracketed name rooted
in a local value is indexing, while one rooted in a generic function is a type
argument list.

Each call checks argument and result types before code generation; the independent
operation checker checks the indirect call as well. Builtins are not named source
functions: wrap a builtin in a typed function when a callable value is needed.

Currently only concrete named functions are values. Callable signatures exclude
references and generator frames. Capture environments and anonymous bodies are
not yet implemented. See the [backlog](../backlog.md) for remaining work.
