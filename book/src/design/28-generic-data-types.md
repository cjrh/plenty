# Generic data types

Classes also accept parameters: `class Cell[T]:` can store `value: T` and define
methods using T. `Cell[u8](7)` returns `Result[Cell[u8], AllocError]`. Bare `self`
uses the current concrete class; an explicit mutable receiver is written
`self: &mut Cell[T]`. Methods, generated or explicit initializers, and destructors
are specialized with the class. Their bodies are checked for each used instance.
Generic classes can be constructed inside generic functions.

Specialization adds no instance allocation beyond ordinary class storage. Moves,
field borrowing, partial initialization cleanup, and custom destruction keep
their ordinary class semantics. Generic methods with their own parameters are
not yet implemented.

An enum can declare type parameters: `enum Choice[T]:` with a variant `Value(T)`.
Use `Choice[u8].Value(7)` and `case Choice[u8].Value(value):` to select a concrete
instance. Type aliases can name complete instances. Different arguments produce
different nominal types, even when no payload mentions the parameter.

Concrete instances are cached per compilation. Fields keep the ordinary enum
storage, allocation, ownership, and drop rules; generic syntax adds no runtime
type lookup or extra allocation. User enum construction still returns Result.
Option and Result retain their separate allocation-free representation.

The initial syntax requires explicit arguments. Type arguments cannot be unit,
references, generators, or closure environments. Recursive data layouts remain
unsupported. Data specialization is capped at 256 instances; nesting at 64 and
concrete names at 16,384 bytes. Bounds on data parameters are not implemented.
