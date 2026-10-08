# Generic data types

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
