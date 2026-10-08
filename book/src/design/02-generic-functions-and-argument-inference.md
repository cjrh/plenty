# Generic functions and argument inference

Functions may declare type parameters, for example
`def identity[T](value: T) -> T:` or `def add[T: IntType](a: T, b: T) -> T:`.
Calls may supply all type arguments explicitly (`identity[list[i64]](values)`)
or infer all of them from arguments (`identity(values)`). Inference structurally
matches parameter types against concrete argument types: `&T` with `&Message`
infers `T = Message`, and `&dict[K, V]` with `&dict[str, u8]` infers both parameters.
Lists, sets, ranges, generators, tuples, Option, Result, and Callable signatures participate too.
Repeated occurrences of a parameter must agree after alias resolution. No
numeric widening, implicit borrowing, protocol implementation search, or runtime
dispatch is introduced. Protocol constraints are checked after inference.

Inference is local to the call and uses the ordinary expression checker. Arguments
are evaluated once in source order. A generic parameter position supplies no
expected type to its argument: unsuffixed integers default to `i64` and floats to
`f64`, independently of other arguments or the expected result. Thus
`add(1u8, 2u8)` works, while `add(1u8, 2)` and `add(1, 2u8)` conflict; use
`add[u8](1, 2)` to guide unsuffixed literals. Nongeneric parameter positions keep
their usual contextual typing. Empty literals and incomplete sum constructors
can require an annotated binding or explicit type arguments. All parameters
must be determined by inputs: output-only/unused parameters require an explicit
type argument list. Partial explicit lists and inference from expected return
types remain deferred. Existing builtin range inference is unchanged.
`IntType` accepts the eight fixed-width integer types, including aliases; it is
a constraint rather than a value type. Unconstrained parameters are also allowed.

`F: Callable[[i64], i64]` accepts a named function or a concrete captured closure
with that exact signature. A consumer borrows `&F` for shared calls or `&mut F`
for mutable calls; ordinary borrow checking enforces the environment's needs.
The constraint does not convert an environment into a code pointer or allocate.
Its signature may mention other type parameters of the same function.
The concrete callback signature supplies inference evidence: `F: Callable[[], T]`
can determine `T` from `F`'s return type. Such evidence must agree with all other
arguments. This does not search for protocol implementations or widen numbers.

The frontend creates one concrete function for each distinct function/type tuple.
Alias-equivalent arguments and recursive calls reuse the same instance. Generated
bodies pass through ordinary type, ownership, and borrow checking. Generic body
operations are checked when instantiated; unused generic bodies are not checked
against every possible type. Module names are resolved in the definition's scope,
and importing a function does not alter which methods are available.

Specialization is requested during typed body lowering and uses a work queue,
with at most 256 concrete generic instances per compilation and the existing
type depth/name limits. Each concrete signature and returned-reference summary
is registered before its body is queued. Explicit, inferred, alias-equivalent,
and recursive calls share one cache keyed by function and resolved types; the
backend still receives only concrete checked operations. This bounds expanding
recursion without maintaining a second expression type checker. Generic classes,
generic methods, reference/unit type arguments, and polymorphic function values
are deferred. A signature can borrow `T` directly using `&T` or `&mut T`.

A generic function can become a concrete [function value](25-function-values.md)
using `identity[u8]`, or using a concrete expected Callable annotation, parameter,
or return type to infer its specialization. That value has one concrete signature.
This contextual selection of a function value does not change the argument-only
inference rules for ordinary generic calls described above.
