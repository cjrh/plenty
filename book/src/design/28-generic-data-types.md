# Generic data types

Classes also accept parameters: `class Cell[T]:` can store `value: T` and define
methods using T. `Cell[u8](7)` returns `Result[Cell[u8], AllocError]`. Bare `self`
uses the current concrete class; an explicit mutable receiver is written
`self: &mut Cell[T]`. Methods, generated or explicit initializers, and destructors
are specialized with the class. Their bodies are checked for each used instance.
Generic classes can be constructed inside generic functions.

Specialization adds no instance allocation beyond ordinary class storage. Moves,
field borrowing, partial initialization cleanup, and custom destruction keep
their ordinary class semantics. Methods may declare their own parameters and
IntType, protocol, or callable constraints. Their arguments determine these
parameters; class parameters are already fixed by the receiver. Method parameters
cannot shadow class parameters. Lifecycle methods cannot add parameters.
Generic method bodies are checked when used. A generic method does not currently
satisfy a protocol's monomorphic method requirement.
Explicit calls use `receiver.method[Types](arguments)`, including class fields
and temporary owned receivers. Supply every method parameter when using explicit
arguments; class parameters are not repeated. Returned references require a named
receiver or class field and retain their ordinary root loans.

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
unsupported. Declaration diagnostics distinguish transparent alias cycles from
recursive data and show the dependency path, including paths through collections
and aliases. Long cycle messages abbreviate intermediate declarations; resolving
long acyclic alias chains uses an explicit worklist.
Data specialization is capped at 256 instances; nesting at 64 and
concrete names at 16,384 bytes. `T: IntType` limits a data parameter to sized
integers. Other data bounds are not yet supported. Imports and `pub` apply to the
declaration and its members; concrete arguments must also be public when exposed
in a public signature, including parameters with no stored field.

Signature shape validation uses an isolated instance cache, so placeholder types
cannot enqueue methods in the real program. Actual specializations drain through
the normal method/body work queue, including instances introduced solely by a
generic function's return signature. Alias-equivalent arguments share layouts
and methods. A regression workload with 300 explicit, inferred, and aliased
method calls produces one method specialization.

Native tests disable allocations after construction across protocol and generic
method calls, moves through Option, generator capture/resumption, and cleanup.
Injected allocation failures verify moved argument cleanup for generic class and
enum construction; failed initialization skips the outer destructor and reclaims
initialized fields. Generic class instances do not yet have C handle names:
export a nongeneric facade class when an ABI needs to own generic storage.
