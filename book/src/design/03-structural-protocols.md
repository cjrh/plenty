# Structural protocols

`protocol Readable:` declares required methods using ordinary `def` signatures
and `pass` bodies. `def read(self) -> str:` implies `self: &Readable`;
`self: &mut Readable` explicitly requires an exclusive receiver. A generic bound
`T: Readable` accepts any class with the required inherent methods. No inheritance,
registration, or import-driven method activation occurs. Calls remain statically
dispatched to concrete methods in the cached specialization.

Every requirement is checked, even if a particular generic body does not call it.
Parameter and return types, arity, and receiver borrowing must match exactly;
parameter names may differ. Normal module visibility applies at the generic
definition, and public signatures cannot expose private protocols or private
types from their requirements. Within requirements, the protocol's own name
denotes the implementing class, including in returned references.

Protocols can declare parameters, for example `protocol Readable[T]:`, and bounds
can supply them as `R: Readable[u8]` or `R: Readable[T]`. See
[parameterized protocols](29-parameterized-protocols.md) for substitution and
visibility rules.

Protocols currently constrain class type arguments only. They are not runtime
value types or existential containers. Fields, protocol inheritance, multiple
bounds, default implementations, generic methods, and associated types are
deferred; lifecycle hooks cannot be requirements. `IntType` remains a separate
builtin numeric-family constraint. Generic bodies are still checked per concrete
instance, so requirements express a minimum interface rather than a separately
type-checked abstract implementation.
