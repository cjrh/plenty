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
Native regression tests disable heap allocation across callable creation,
selection, copies, calls, and nested inline Result/Option/range returns.

The called function can be selected by an arbitrary expression: `choose()(n)`, `(f)(n)`,
`handlers[index](n)`, and `record.callback(n)` all work. A bracketed name rooted
in a local value is indexing, while one rooted in a generic function is a type
argument list.
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
scope. Capturing a surrounding local is rejected, including locals that shadow a
module function. Indented anonymous bodies inside delimiters are unsupported.

Callable signatures exclude generator frames. Capturing functions have their own
concrete environment type; they do not convert to a code-only `Callable`.

An explicit capture list, `def [offset, values](index: i64) -> i64:`, transfers
those bindings into inline environment storage. Scalars copy as usual; owners
move. The body can observe and borrow captures, but cannot move them out. Calling
the closure borrows its environment, so calls can repeat. Assignment moves the
closure; `copy` is unavailable. Captured owners drop in reverse capture order when
the environment leaves scope. Creating and moving the environment do not allocate.
`def [mut count, values](...)` allows changing `count` while keeping `values`
read-only. The source binding need not be mutable: the new environment owns it.
Such a closure requires an exclusive environment borrow and a `mut` binding to
call. Assignment to a captured scalar changes the environment slot; methods on
captured owners obey the same explicit mutation permissions.
`def [&values](...)` borrows a surrounding binding without taking ownership.
Shared capture loans remain live through later calls and moves, preventing changes
to or destruction of the owner. They can end after the last use. Borrowing
closures cannot escape their function, enter enum/container storage, or be
suspended in generators. They can be passed by reference and can borrow another
closure: an environment loan keeps all transitive captured loans live through the
call, including evaluation of later arguments. Passing a borrowing closure by
value is currently rejected.
`&mut` captures exclusively borrow a mutable source binding. Calls require a
mutable closure binding and can update the original value; other reads, writes,
and borrows of that source are excluded while future closure uses keep the loan
live. Explicit reference arguments must also be disjoint from captured loans.
`Closure[[parameter types], result]` constrains a concrete environment's signature.
Owned closures can be returned and passed by reference with this annotation; the
compiler infers the producer identity and specializes consumers for its layout.
Different closure expressions do not unify merely because their signatures match.
The annotation introduces neither boxing nor a dynamically sized environment.
An owning return moves captures into caller-provided storage.
Currently closures must be called through a named binding, cannot return
references or yield, and cannot own other restricted storage.
See the [backlog](../backlog.md) for remaining work.

Indirect calls in tail position use native tail calls under the same cleanup and
borrowing restrictions as direct calls. Calls involving inline argument/result
storage retain their frame; observable destructors run after the callee returns.
