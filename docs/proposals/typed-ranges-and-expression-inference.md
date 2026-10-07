# Typed ranges, generic calls, and expression-local inference

Status: recommended design, 2026-10-06; not implemented. This responds to the
typed-range discussion after module support (`8ee5c64`). The current compiler
accepts numeric suffixes, but ranges still produce `i64` and numeric literals
do not yet take their types from context.

## Recommendation

Yes: use `range[u8](8)` and support contextual inference from `list[u8]`.
Square brackets should be the ordinary explicit type-argument syntax for
generic functions, including qualified calls such as `numbers.range[u8](8)`.
No extra separator is needed before the brackets.

The three examples below should all produce `list[u8]`:

```text
squares = [n * n for n in range[u8](8) if n % 2 == 0]
squares: list[u8] = [n * n for n in range(8) if n % 2 == 0]
squares = [n * n for n in range(8) if n % 2u8 == 0]
```

The first states the range's type directly; the second states the desired
collection type. Those are the clearest forms to teach. The third follows from
the same inference rules, although placing the type choice in a filter makes
it less obvious to a reader.

Numeric suffixes such as `2u8`, `42i32`, `3.5f32`, and `1e-3f64` **already work**.
Keep them as exact type constraints. The new convenience is contextual typing
of unsuffixed literals, not implicit numeric conversions.

## Infer before defaulting or generating operations

The current compiler lowers a comprehension's iterable before its result
expression and assigns concrete numeric literal types immediately. It cannot
make the second and third examples work just by accepting a new call spelling.
Introduce a typed-expression inference step before lowering to operations:

1. Instantiate a generic signature with fresh inference variables, or substitute
   the explicit type arguments supplied by the caller.
2. Collect constraints from declared types, explicit literal suffixes, argument
   and result types, operators, branches, and comprehension bindings/filters.
3. Solve equalities and check bounds. Only then default unconstrained integer
   variables to `i64` and floating-point variables to `f64`.
4. Validate literal representability and lower concrete types to existing IR.

For the annotated comprehension, the result element must be `u8`. Multiplication
has equal operand/result types, so both uses of `n` must be `u8`. That determines
the range's element type. Its stop literal `8`, the remainder divisor `2`, and
the comparison operand `0` can then be checked as `u8` literals.

For the suffix example, `%` requires equal operand types: `2u8` constrains `n`
to `u8`. That propagates to the iterable and multiplication. Comparison produces
`bool` but still requires its numeric operands to have the same type.

No constraint source silently overrides another. For example, an annotated
`list[u8]` conflicts with `range[u16](8)` when the element expression is `n * n`.
An explicit cast in the element expression can make that intentional instead.

Keep the inference region small: an initializer, an assignment RHS, or a return
expression, including nested calls, comprehensions, and expression-valued
branches. A function's declared return type constrains its tail expression and
each explicit return. Function interfaces still require parameter and return
types. Do not infer signatures from callers or inspect callee bodies to infer
call-site types.

Once an unannotated local initializer has been resolved and defaulted, its type
is fixed. A later use does not retroactively specialize it:

```text
values = range(8)                     # defaults to range[i64]
squares: list[u8] = [n * n for n in values]  # error
```

Use `values = range[u8](8)` at the earlier binding instead. Likewise, a declared
return type constrains a comprehension used directly as the final expression;
it does not reach back through an already defaulted local initializer. This
boundary keeps diagnostics predictable and avoids whole-function inference.

An output annotation also cannot determine every input in an arbitrary chain:

```text
values: list[u8] = [u8(n) for n in range(8)]
```

Here the cast explicitly separates input and output types. The range can still
default to `i64`. Types flow through the known signatures of operations, not
through guesses about their behavior.

## Literal context is not conversion

Under the proposed rules, `x: u8 = 8` and `x: f32 = 3.5` become valid because
the literal is initially unresolved. However:

```text
limit: i64 = 8
values = range[u8](limit)  # error: an existing i64 is not a u8
values = range[u8](8i64)  # error: an explicit suffix fixes the type
values = range[u8](300)   # error: the literal cannot fit in u8
```

Retain explicit casts for numeric conversions. Initially keep integer-shaped
unsuffixed literals in the integer family and decimal/exponent literals in the
floating-point family. To spell a floating-point whole number, use `1.0` or an
explicit suffix such as `1f32`, which is already supported.

Contextual literals are a deliberate change to the current language contract.
Some examples that currently teach `small: f32 = 1.5` as a type error must become
successful examples when this is implemented. Transparent aliases should supply
the same context as their targets; they still need not create new literal suffixes.
Integer arithmetic remains checked at its inferred width: `u8` multiplication
must not secretly widen just because the expression appears in a comprehension.

## `IntType` is a compile-time constraint

The sketched declaration is workable:

```text
def range[T: IntType](stop: T) -> range[T]:
    ...
```

Here the return spelling describes a concrete built-in iterable. The example
is a signature sketch, not a requirement to implement this intrinsic in Plenty
source immediately. A user-written yielding function would naturally return
`Generator[T]` using the existing generator model once generic functions exist.

Define `IntType` initially as a compiler-known constraint admitting exactly
`i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, and `u64`, including aliases of
those types. It excludes `bool` and floats. It is neither a runtime sum type nor
a type alias for a union. Its contract grants the operations required for integer
code: comparisons, checked arithmetic, and representable integer literals.

This does not require user-defined arithmetic protocols in the first release.
Ordinary structural protocols remain the plan for user-defined method contracts.
Keep these mechanisms distinct; a method name alone cannot establish built-in
integer representation or arithmetic semantics.

Do not introduce a runtime-erased `Iterator[T]` merely to express `range`.
The existing language distinguishes concrete iterable values and generators;
an iterator protocol can later describe shared operations and borrowing rules.
Returning a concrete range preserves cheap repeated iteration and indexing,
which differ from consuming a generator.

Initially specialize reachable generic calls and reuse an instance for each
declaration/type-argument combination. Check generic bodies against their bounds.
Do not add overload search, implicit trait imports, compile-time arbitrary
execution, or unconstrained specialization as part of typed ranges.

## Range boundaries and descending unsigned iteration

The one-argument form is straightforward: both implicit zero and `stop` have
type `T`, and yielded elements have type `T`. Inferring `T` from a typed argument
should work too: `range(8u8)` implies `range[u8](8)`.

Recommend that the three-argument form use `T` for `start` and `stop`, but a
separate signed `i64` for `step`. This allows:

```text
range[u8](8, 0, -1)  # yields u8 values 8 through 1
```

The step is a displacement, not an element. Zero remains invalid. This deliberately
limits the step magnitude to the supported `i64` domain even for `u64` elements;
larger displacements can be a later extension. The concrete intrinsic may retain
its existing one-, two-, and three-argument forms without committing the language
to general overloads or default arguments yet.

Keep exclusive endpoints in `T` initially. Therefore `range[u8](256)` is a
compile-time error, and an ascending exclusive `u8` range cannot include `255`.
Do not silently widen the bound or wrap it to zero. A future inclusive-range API
can cover the full domain; a separately typed bound would be an alternative design
but would no longer have the simple `stop: T` signature above.

Iteration must check whether another element exists before an increment that
would overflow the element type. For example, `range[u8](254, 255, 2)` yields
only `254` and terminates normally. Use widened or checked unsigned-magnitude
calculations for bounds, differences, length, and stepping, including `u64` and
the most negative `i64` step. Do not lower these cases through signed `i64`
element arithmetic. If a length cannot fit the language's length result type,
report the limit rather than wrapping; specify that behavior alongside the API.

## Reserve space for functions as values and closures

Generic application and indexing share brackets. Resolve `f[T](x)` as generic
application when `f` names a generic callable; retain `callbacks[i](x)` for an
indexed callable value once functions become values. The parser must preserve
this distinction for name/type resolution, rather than assuming that every
bracket followed by a call denotes type arguments. Type arguments resolve in
the type namespace, with the same explicit import rules as annotations.

Multiline anonymous functions are compatible with this direction. A promising
spelling, deliberately still provisional, is an anonymous `def` expression:

```text
transform = def(value: i64) -> i64:
    adjusted = value * 2
    adjusted + 1
```

Keep its parameters and return type explicit, like a named function. Expected
return types then constrain literals and generic calls in its body. Captures
need no separately repeated type declarations: their types come from bindings.

An initial closure model should distinguish:

- Read-only borrowing of captured owners.
- Exclusive borrowing when the closure mutates a captured owner.
- Explicit ownership capture for a closure that outlives the surrounding scope.
- Captures consumed during a call, making the closure callable at most once.

Exact capture syntax remains open. Do not infer an invisible deep copy, or decide
capture ownership by whether a later compiler pass happens to let a closure
escape. A borrowing closure cannot outlive its owners, and captured borrows must
remain active while future calls may use them. Owned captures drop with the
closure, subject to moves and consumption. Checking these rules will require
extending today's borrow model, not just lowering a function plus an environment.

Use concrete environment layouts and static call contracts for generic callable
parameters where possible. Avoid mandatory heap allocation, reference counting,
or dynamic dispatch for every closure. A heterogeneous collection of callbacks
would need an explicit representation decision later.

Anonymous suites nested inside call arguments also require a parser design:
the current lexer suppresses indentation inside parentheses. Preserve the goal
of multiline anonymous functions in expression positions without claiming the
assignment example above settles nested suite termination or comma placement.

## Suggested delivery

1. Add contextual numeric literal inference and a typed-expression phase with
   deferred defaults. Cover initializers, calls, returns, branches, filters, and
   comprehension elements, including conflicting and unrepresentable constraints.
2. Add explicit function type arguments and typed range representation/lowering,
   using the same generic-call model intended for user functions. Test unsigned
   descent, boundary termination, maximum-width values, and empty ranges.
3. Expand to explicitly bounded generic functions and cached instantiation.
   A first `IntType` constraint can precede general structural protocols.
4. Add callable values and closure capture checking, with a separately reviewed
   multiline expression grammar. The range work must not preclude these features.

This is a proposed refinement of the generics plan, not an implementation of
these examples or an automatic replacement of the existing `?`/allocation
roadmap. The learning guide should continue to teach current behavior until
the compiler and runnable examples migrate together.
