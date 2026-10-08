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

Class constructors infer parameters from their input arguments, using generated
field order or an explicit initializer's signature. `Cell(7u8)` means
`Cell[u8](7)`; `Cell(7)` uses i64. Output-only or phantom parameters require
explicit arguments. A result annotation does not change argument inference.
Arguments evaluate exactly once in source order. Enum qualifiers still require
explicit arguments.

Generic functions infer arguments from
data instances: `def read[T](value: &Cell[T]) -> T` learns T from `Cell[u8]`,
including through aliases, nested collections, and repeated parameters. Inference
checks the nominal declaration as well as its arguments; matching fields alone
do not make unrelated classes interchangeable.

Type arguments cannot be unit,
references, generators, or closure environments. Recursive data layouts remain
unsupported. Data specialization is capped at 256 instances; nesting at 64 and
concrete names at 16,384 bytes. `T: IntType` limits a data parameter to sized
integers. Other data bounds are not yet supported. Imports and `pub` apply to the
declaration and its members; concrete arguments must also be public when exposed
in a public signature, including parameters with no stored field.
