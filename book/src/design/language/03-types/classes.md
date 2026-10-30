# Classes and construction

A class declares typed fields and associated methods. It has no inheritance,
dynamic attributes, class variables, properties, or runtime method lookup.

```plenty fragment
class Point:
    x: i64
    y: i64

    def squared_length(self) -> i64:
        self.x * self.x + self.y * self.y
```

Without `__init__`, the generated constructor takes fields in declaration order.
`Point(3, 4)` and `Point.new(3, 4)` return `Point` directly. Construction itself
allocates nothing; arguments and initializer operations retain their own error
contracts. `new` is a reserved class member.

An explicit `__init__` overrides the generated constructor and returns either
`()` or `Result[(), AllocError]`. The fallible form makes both constructor spellings
return `Result[Class, AllocError]`. Other initializer error types are unsupported.
Fields may contain classes, enums, tuples, collections, and resolved owned
closures, but not references, generators, or unit.

Every successful initializer exit must initialize every field. Branches merge
their definitely initialized fields; a loop alone cannot establish initialization
because it may run zero times. Initialized fields may be read. Borrowing or
passing the whole partially initialized instance is rejected, and initialization
cannot be delegated to another method. Constructors accept named arguments,
using field names for generated constructors and parameter names for explicit
initializers. Explicit initializer parameter defaults are inherited by both
constructor spellings. See [call rules](../06-functions/01-signatures-and-calls.md).
Field defaults, constructor overloading, and static methods are unsupported.

Failed initialization destroys initialized fields and remaining owned arguments
and skips the class's destructor. The destructor becomes active only after
successful initialization; see [cleanup](../05-ownership/index.md).

Classes can define [arithmetic operator hooks](../04-expressions/operators.md)
such as `__add__` and `__neg__`, compiled as direct method calls.

Class identity is nominal. Structural equality compares fields and printing uses
`Point(x=3, y=4)`; neither invokes user-defined magic methods. Automatic equality
and formatting reject recursive data. Instance moves, field access, and explicit
duplication follow [ownership](../05-ownership/index.md); receiver signatures
follow [methods](../06-functions/index.md), and constructor access follows
[visibility](../02-source/visibility.md).

A [class pattern](../04-expressions/matching.md#class-patterns) such as
`case Point(x, y=0):` takes a class apart in `match`, written like the
generated constructor with every field named once.

## One-field classes

A class with exactly one field and no `__del__` is represented as that field.
Wrapping a value in such a class, reading its field, or moving the field out
does not copy or allocate, so a wrapper such as `class Handle: _file: File`
costs the same as the bare `File`. The field's type is unrestricted: a scalar,
`str`, a collection, an enum, or another class. Nothing in the source changes:
the class keeps its name, methods, equality, and printing
(`Handle(_file=...)`), and the field keeps its
[visibility](../02-source/visibility.md). A one-field class with `__del__` keeps a separate
record because its cleanup state needs storage. See
[value representations](../../toolchain/02-architecture/values.md).

A field whose type is another class stores that class's fields inside the
owner, at a fixed position. Reading `path.start.x` costs the same as reading a
field of `path` itself, and moving or returning `path` copies its bytes once,
with no per-field address work. This is not visible in source: nested fields
still move, borrow, and run `__del__` as other fields do.

## Copy classes

`@copy` on the line before `class` declares a copy class. Its instances copy on
ordinary value use, as copyable tuples and enums do: assignment, owned argument
passing, return, collection insertion, capture, and by-value receivers
(`self: Point`) all leave the source usable. Copying duplicates the inline
fields and never allocates; string fields share immutable storage. `copy(value)`
still accepts the instance. Other classes remain affine.

```plenty
@copy
class Point:
    x: i64
    y: i64

def main() -> Result[(), IoError]:
    p = Point(1, 2)
    mut q = p
    q.x = 5
    print(p, q)?
    Ok(())
```

```output
Point(x=1, y=2) Point(x=5, y=2)
```

The declaration is checked as a whole:

- Every field must copy: a scalar, `str`, another `@copy` class, or a tuple or
  enum whose components copy. The error names the field and its type.
- The class cannot define `__del__`, because each copy would run it.
- The class cannot have type parameters. No bound states that a type argument
  copies, so the field rule could not be checked at the declaration.

Equality, printing, field access, and matching are unchanged. Classes,
including copy classes, are not yet dictionary keys or set elements; see the
[user type hashing proposal](../../../proposals/user-type-hashing.md).

### Annotations

An annotation is `@name` alone on a line directly before a declaration. The
set is closed and compiler-known; there are no user decorators. The
annotations are `@copy` and `@newtype`; both apply only to class declarations,
including `pub class`. An unknown name, arguments, a repeated annotation, or an
annotation before any other declaration, member, or statement is an error.

`@newtype` makes a class forward its field's operators; see
[newtype forwarding](../04-expressions/operators.md#newtype-forwarding). The
class must declare exactly one field and cannot define `__init__`. It can
have type parameters (`@newtype class Wrapper[T]: value: T`) and `__del__`.

`@copy` and `@newtype` are independent. `@newtype` does not imply `@copy`: a
`@newtype` class over `i64` still moves on assignment unless it is also
`@copy`. Forwarded operators borrow their operands, so `a + a` and reusing `a`
after `a + b` work either way. A class can carry both annotations, in either
order, with the same meaning:

```plenty
@newtype
@copy
class Seconds:
    value: f64

def main() -> Result[(), IoError]:
    a = Seconds(1.5)
    b = a
    print(a + b)?
    Ok(())
```
```output
Seconds(value=3.0)
```

Neither annotation changes layout or printing. Every class with one field and
no `__del__` uses the [one-field representation](#one-field-classes), with or
without `@newtype`, and a `@newtype` value prints as its class
(`Seconds(value=3.0)`). Annotation names follow
`@` and do not reserve words: `copy` remains an ordinary name, and the builtin
`copy(value)` is unaffected.

