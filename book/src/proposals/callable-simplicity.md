# Callable surface review

Plenty needs callable values for callbacks, small local functions, and owned
resource handoff. This review separates representation from ownership and avoids
making every capability a prerequisite for writing an ordinary closure.

## Decisions

| Surface | Concrete need | Decision |
| --- | --- | --- |
| `def(...)` | A multiline function expression | Keep; the same typed function body as a named function |
| `def [captures](...)` | State without implicit sharing or allocation | Keep explicit captures; infer the environment type locally |
| `value`, `mut value`, `&value`, `&mut value` | Own or borrow state, with ordinary mutability | Teach as the existing ownership and mutability rules composed together |
| `Callable[[A], B]` as a value type | Store different named functions in one registry | Keep the code-address representation; it must not silently box captured state |
| `F: Callable[[A], B]` | One reusable callback API for named and captured functions | Prefer this for generic consumers; infer `F` at calls |
| `Closure[[A], B]` | Describe a factory's concrete environment result | Keep for factories and specialized interfaces; remove it from the default consumer lesson |
| `def once [captures]` | Transfer captured owners out of a callback | Keep the explicit consuming contract; do not infer it from body changes |
| `OnceClosure[[A], B]` | Describe a factory returning a consuming environment | Keep as an advanced factory annotation |
| `F: OnceCallable[[A], B]` | A task API that accepts all call modes and consumes its callback | Keep as an advanced constraint, never a stored value type |

There are two representations: a code address and a concrete environment. There
are two invocation contracts: borrowing an environment for reuse and consuming
it. The public annotations describe these differences; they do not introduce
four interchangeable forms of dynamically dispatched object.

Collapsing every value into `Callable` would require either a uniform erased
environment with explicit storage, or a type spelling that sometimes denotes a
code address and sometimes a producer-specific layout. Neither makes storage
costs easier to predict. Keep the distinction visible at interfaces while letting
ordinary local bindings infer it. `OnceCallable` could not be an ordinary
method-only protocol today: invocation and consuming `self` are compiler-checked
operations, and the existing protocols do not describe those operations.

Inferring `once` from moving a capture out would make a body edit change whether
callers can invoke it again. An explicit word gives a local ownership contract,
like the difference between an owned parameter and a reference. A resource
handoff needs that distinction; an ordinary read-only callback does not.

## What belongs in libraries

A registry of named callbacks needs only a collection of `Callable` values.
A callback plus explicit state can already be a generic class with ordinary
methods. Filtering, dispatching, and adapting these callbacks are library code;
they need no new invocation syntax or builtin registry type. Captured environment
storage still needs a representation and lifetime contract before library code
can build a heterogeneous closure registry.

The teaching path is named function values, then ordinary inferred closures,
then a reusable generic callback API. Factory annotations, borrowed captures,
and consuming callbacks are optional extensions for concrete use cases. The
[learning guide](../tutorial/index.md) links that shorter path. Existing advanced
lessons remain referenceable, so simplifying the entry path loses no examples.

This review does not claim that the current names must be permanent. It records
why each retained distinction is necessary today, and replaces unnecessary
consumer-specific annotations with the common generic interface.
