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

Generic function type arguments can be unit `()`, including when inferred from
`Result` or `Option` payloads. Substituted signatures and bodies retain ordinary
unit restrictions: unit can be a return type or built-in enum payload, but not
a direct function parameter or stored collection element. A `&T` or `&mut T`
parameter instantiated with `()` borrows unit, so a helper such as
`std.test.is_err` accepts a `Result[(), E]`. Explicit calls and specialized
function values also accept `()`.

Generic function type arguments cannot be references. A signature can borrow
`T` with `&T` or `&mut T`. A callable-constrained parameter can borrow a
concrete closure while keeping its capture loans live across the call. Data and
protocol arguments have their additional restrictions on their own pages.

## Diagnostics from instances

A generic body is not checked where it is declared. Each used instance is
checked once, and an error found in it is reported at the use that first required
the instance, in code that is not itself a generic instance. A note gives the
position in the definition, the instance being checked, and the reason:

```text
main.plenty:7:9: cannot instantiate `outer[i64]`: its definition is rejected
  pairs.plenty:5:13: note: in instantiation `pairs.scaled[i64]`: unknown binding `pairs.factor`
```

The first line names the instance the use asked for. The note names the instance
whose definition failed; the two differ when generic code requires further
instances, and the instances between them are not listed. Further notes belong
to the reason. Names longer than 96 characters are cut short.

When the body is rejected only because of a type argument, the first line names
that argument instead. Three causes are recognized: an implicit copy of an
affine argument (moving it out of a field, an element, or a reference, or using
a moved binding of it), an operator the argument does not support, and a method
the argument does not have:

```text
main.plenty:8:9: `outer[list[i64]]` requires `pairs.twice[list[i64]]`, which copies `T`, but `list[i64]` cannot be copied
  pairs.plenty:2:13: note: in instantiation `pairs.twice[list[i64]]`: use of moved binding `value`
  pairs.plenty:2:6: note: `value` is moved here
```

The note then describes the rejected operation without advice for changing the
body, which the caller may not own. A cause is attributed when the rejected type
is exactly one parameter's argument; otherwise, including when two parameters
have that argument, the first form is used. The match is by type, so a body that
names the argument's type itself, such as a `list[list[i64]]` field read for
`T = list[i64]`, is attributed to `T`.

This covers generic functions, generic methods, functions written inside them,
and the methods of a generic class or enum instance. A method of an instance is
checked when it is first used: called, including by a constructor call for
`__init__`, or required by a [protocol bound](protocols.md) that the instance
satisfies. A `__del__` method is checked when the instance is first required,
because constructing a value arms it. An instance can therefore exist although
one of its methods would be rejected for it, as long as that method is not used:

```text
main.plenty:9:13: `Cell[list[i64]].get` copies `T`, but `list[i64]` cannot be copied
  main.plenty:5:9: note: in instantiation `Cell[list[i64]].get`: cannot move out of a field
```

A method required through a protocol bound is reported at the bounded use,
naming the bounded instance, such as `accept[Holder[Guard]]`. Unused methods are
not compiled for the instance either.

Since bodies are checked only per instance, a body error that no type argument
causes, such as an unknown name, is reported in the first form, and not at all while
the declaration is unused. Errors in the declaration itself (type parameters,
constraints, signature types) and uses that do not satisfy a constraint are
reported at one location. Instance field types, constraints on data
arguments, and substituted method signatures are checked when the instance is
first required.

### Checking every method

Checking per use keeps compilation fast while code changes, but a library's
tests then check a method for a type argument only if they use it. The check
modes accept `--check-all-methods`
([`plenty --check`, `--check-module`](../../toolchain/01-building/commands.md),
and [`plenty check`](../../toolchain/01-building/packages.md)): after the used
methods, every remaining method of every class and enum instance the program
creates is checked as if it were called, including methods of standard-library
types. Such an error has no use, so it is reported where the program first
required the method's type, such as a constructor call or a type annotation,
with a note naming the flag:

```text
main.plenty:8:9: `Cell[list[i64]].get` copies `T`, but `list[i64]` cannot be copied
  main.plenty:4:9: note: in instantiation `Cell[list[i64]].get`: cannot move out of a field
  note: `Cell[list[i64]].get` is checked because of --check-all-methods; the program does not use it
```

The flag checks only instances: a generic class or enum with no instance stays
unchecked, since its bodies are checked with concrete type arguments and there
are none. Compiling does not accept the flag, so it never changes generated code
or the [compile cache](../../toolchain/02-architecture/cache.md).
