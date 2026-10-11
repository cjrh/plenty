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

Functions and methods accept unconstrained parameters, `IntType`, `FloatType`,
structural protocol constraints, and callable constraints. `IntType` accepts the
eight fixed-width integer types and `FloatType` accepts `f32` and `f64`,
including aliases; each is a constraint rather than a value type. No constraint
admits both integers and floats; an unconstrained parameter does, because a
body is checked for each instance. Callable capabilities follow [functions](../06-functions/index.md).

[Argument inference](inference.md) determines parameters from inputs or a complete
explicit argument list. [Generic data and methods](data.md) retain nominal
identity. [Protocols](protocols.md) describe exact structural interfaces without
inheritance or registration.

Generic function type arguments cannot be references or unit. A signature can
borrow `T` with `&T` or `&mut T`. A callable-constrained parameter can borrow a
concrete closure while keeping its capture loans live across the call. Data and
protocol arguments have their additional restrictions on their own pages.

## Diagnostics from instances

A generic body is not checked where it is declared. Each used instance is
checked once, and an error found in it is reported at the use that first required
the instance, in code that is not itself a generic instance. A note gives the
position in the definition, the instance being checked, and the reason:

```text
main.plenty:9:9: cannot instantiate `outer[list[i64]]`: its definition is rejected
  pairs.plenty:2:13: note: in instantiation `pairs.twice[list[i64]]`: use of moved binding `value`
  pairs.plenty:2:6: note: `value` is moved here
```

The first line names the instance the use asked for. The note names the instance
whose definition failed; the two differ when generic code requires further
instances, and the instances between them are not listed. Further notes belong
to the reason. Names longer than 96 characters are cut short.

This covers generic functions, generic methods, functions written inside them,
and the methods of a generic class instance. All methods of a class instance are
checked when the instance is first required, whether or not they are called.

Since bodies are checked only per instance, a body error that no type argument
causes, such as an unknown name, is reported the same way, and not at all while
the declaration is unused. Errors in the declaration itself (type parameters,
constraints, signature types) and uses that do not satisfy a constraint are
reported at one location.
