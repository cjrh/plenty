# Classes and construction

A class declares typed fields and associated methods. It has no inheritance,
dynamic attributes, class variables, properties, or runtime method lookup.

```python
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
