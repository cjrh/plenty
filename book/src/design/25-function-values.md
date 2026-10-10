# Function values

A named function can be assigned, passed, and returned as a value. Its type is
`Callable[[parameter types], result type]`; parameter names are not part of type
identity. A no-argument function uses `Callable[[], T]`, and a function returning
unit uses `Callable[[T], ()]`. Aliases can name these signatures.

Generic calls infer parameters inside callable inputs and results, for example
`def apply[T](f: Callable[[T], T], x: T) -> T`. All argument evidence must agree;
the compiler does not convert a concrete callable to a different signature.
Callable values can themselves be concrete generic arguments.

To take a generic function as a value, supply its type arguments without a call,
such as `identity[u8]`. This uses the same specialization cache and bound checks
as direct generic calls. A bare generic function name can instead be specialized
by a concrete expected Callable from an annotation, parameter, or return type.
Both parameter and result positions contribute evidence. Unconstrained parameters
still require explicit type arguments; this does not add higher-rank polymorphism.

Function values are copyable code addresses. Creating, passing, returning, and
calling them needs no heap allocation. Calls use Plenty's internal ABI, including
caller-provided storage for inline results. They are not C function pointers.
Allocations performed by the called function retain their ordinary Result API.
An ordinary generic class can pair explicit owned state with a named callback
that borrows that state. Collections of these records form stateful registries
without requiring stored closure environments; see the
[runnable registry example](../tutorial/76-store-stateful-callbacks.md).
Native regression tests disable heap allocation across callable creation,
selection, copies, calls, and nested inline Result/Option/range returns.

The called function can be selected by an arbitrary expression: `choose()(n)`, `(f)(n)`,
`handlers[index](n)`, and `record.callback(n)` all work. Brackets after a name
or dotted path that starts at a local binding index it, even when the binding
shares its name with a generic function; brackets after a declaration or module
path are a type argument list. The index is any single expression, so
`handlers[order[i]](n)` works. After a field or method name on a value, as in
`record.callbacks[order[i]](n)`, the brackets index when their contents start at
a local binding and are method type arguments otherwise.
Evaluation proceeds once, left to right: the callee expression first, then its
arguments. Propagation from a later argument releases pending owned arguments.
Known callable result types also contextualize surrounding numeric literals.

Function values display as `<function>`. Equality compares native code addresses;
it does not compare function behavior. They cannot be dictionary keys or set
elements. Imports and public signatures retain normal visibility rules, including
types nested inside callable signatures.

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

Multiline anonymous functions use `def(parameters) -> ResultType:` followed by an
indented suite. They lower to ordinary native functions with no environment.
They can be assigned, returned, and nested; enclosing generic type parameters are
substituted. Their own value parameters and local bindings form an independent
scope. Implicit capture of a surrounding local is rejected, including locals that
shadow a module function. Indented anonymous bodies inside delimiters are unsupported.

Callable signatures exclude generator frames and closure environments. A named
Callable value can be borrowed through an explicit reference parameter. For
functions with explicit captures, see [closure environments](26-closures.md).
For one generic API that accepts both representations, use a `Callable` constraint
as described in [generic functions](02-generic-functions-and-argument-inference.md).

Indirect calls in tail position use native tail calls with the same cleanup order
and borrowing restrictions as direct calls: the caller's locals are dropped
before the transfer. Calls involving inline argument/result storage retain their
frame but drop its locals first.
