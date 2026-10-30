# Generic data and methods

```plenty fragment
class Cell[T]:
    value: T

    def read(self) -> T:
        self.value

enum Choice[T]:
    Value(T)
```

`Cell[u8](7)` constructs a `Cell[u8]` directly using the ordinary class rules.
Class constructor parameters can be inferred from generated field order or an
explicit initializer's signature: `Cell(7u8)` selects `u8`, while `Cell(7)` selects
`i64`. Output-only or phantom parameters need explicit arguments; a result
annotation does not influence argument inference.

A constructor's enum qualifier requires explicit arguments:
`Choice[u8].Value(7)` constructs a concrete instance. A
[pattern](../04-expressions/matching.md) takes none, because the matched value
supplies them: `case Choice.Value(value):` matches it. Aliases can name
complete instances. Different arguments produce different nominal identities
even when a parameter is absent from every payload.

Bare `self` uses the current concrete class or enum; an explicit mutable receiver is
`self: &mut Cell[T]` or `self: &mut Choice[T]`. A by-value receiver is
`self: Cell[T]` or `self: Choice[T]` (the current-owner spellings `Cell` and
`Choice` also infer their declared parameters). Ordinary methods may declare their own parameters and
integer, protocol, or callable constraints. Data parameters are already fixed
by the receiver; method parameters cannot shadow them. Lifecycle methods cannot
add parameters.

Explicit method calls use `receiver.method[Types](arguments)`. Supply every method
parameter without repeating data parameters. Inferred method arguments follow
ordinary [inference](inference.md). Fields and temporary owned receivers support
generic method calls; returned references retain their ordinary named-receiver
and lifetime restrictions. Brackets beginning with a local binding index a
callable field instead, as in `table.callbacks[index](argument)`.

Data parameters accept `IntType`, `FloatType`, `Callable`, and `OnceCallable` constraints;
protocol bounds on data are unsupported. Callable signatures may mention other
data parameters and contribute constructor inference evidence. Explicit instances
and aliases are checked too; unused declaration bounds still require valid
signature shapes.

Data type arguments cannot be unit, references, or generators. Resolved owned
closures can be arguments; see [stored callables](../06-functions/index.md).
Generic fields, construction, moves, and destruction keep their ordinary data
semantics, including [recursive storage](../03-types/recursive-data.md).

Public instances must expose public concrete arguments, including phantom
parameters and types named only in callback constraints. Declaration and member
visibility apply normally. A generic method does not currently satisfy a
protocol's monomorphic method requirement.
