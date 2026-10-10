# Generics and constraints

Functions, classes, enums, and methods can declare type parameters:

```python
def identity[T](value: T) -> T:
    value

def add[T: IntType](a: T, b: T) -> T:
    a + b
```

Concrete uses preserve ordinary type and ownership rules. Generic bodies are
checked for each used concrete instance. Calls remain statically dispatched;
generic syntax adds no runtime type lookup or allocation.
The toolchain documents [specialization limits and caching](../../toolchain/02-architecture/emission.md).

Functions and methods accept unconstrained parameters, `IntType`, structural
protocol constraints, and callable constraints. `IntType` accepts the eight
fixed-width integer types, including aliases; it is a constraint rather than a
value type. Callable capabilities follow [functions](../06-functions/index.md).

[Argument inference](inference.md) determines parameters from inputs or a complete
explicit argument list. [Generic data and methods](data.md) retain nominal
identity. [Protocols](protocols.md) describe exact structural interfaces without
inheritance or registration.

Generic function type arguments cannot be references or unit. A signature can
borrow `T` with `&T` or `&mut T`. A callable-constrained parameter can borrow a
concrete closure while keeping its capture loans live across the call. Data and
protocol arguments have their additional restrictions on their own pages.
