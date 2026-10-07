# Function values

A named function can be assigned, passed, and returned as a value. Its type is
`Callable[[parameter types], result type]`; parameter names are not part of type
identity. A no-argument function uses `Callable[[], T]`, and a function returning
unit uses `Callable[[T], ()]`. Aliases can name these signatures.

Generic calls infer parameters inside callable inputs and results, for example
`def apply[T](f: Callable[[T], T], x: T) -> T`. All argument evidence must agree;
the compiler does not convert a concrete callable to a different signature.
Callable values can themselves be concrete generic arguments.

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

Reference parameters retain ordinary explicit `&` / `&mut` argument syntax.
The borrow checker keeps these loans live through the indirect call. Owned
arguments move exactly as they do in direct calls, and unit results leave no value.
Returned references require exactly one reference parameter, which supplies their
lifetime. A mutable result requires a mutable reference input. An indirect call
borrows that whole origin; it does not use a named function's more precise field
projection summary. The loan remains live until the returned reference's last use.

Currently only concrete named functions are values. Callable signatures exclude
generator frames. Capture environments and anonymous bodies are
not yet implemented. See the [backlog](../backlog.md) for remaining work.
