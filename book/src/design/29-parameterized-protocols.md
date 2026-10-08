# Parameterized protocols

`protocol Readable[T]:` describes a family of structural requirements. A method
such as `def read(self) -> T` has the concrete return type supplied by a bound
`R: Readable[u8]`. The implementing class does not declare an inheritance or impl
relationship. Its accessible methods must match exactly, including receiver
mutability, parameter types, and return type.

Function and method constraints may mention other parameters, for example
`def read[T, R: Readable[T]](source: &R) -> T`. Once an argument determines R,
its concrete method signatures also supply evidence for T. Nested parameter
types and return types participate. Evidence from ordinary arguments, callable
bounds, and protocol bounds must agree. The compiler follows these fixed
signatures until no further parameters are learned; it never searches for a
class or an implementation. A protocol parameter absent from every requirement
still needs explicit arguments. Requirements are substituted and
checked before the consumer is specialized. Protocols do not introduce runtime
objects, dispatch tables, boxing, or method-import side effects.

Bare `self` borrows the implementing class. Mutable requirements may write
`self: &mut Writable[T]` or `self: &mut Writable`. Public contracts must expose
public concrete argument types; required methods must be accessible from the
bound's defining module. Importing a protocol does not add methods to a class.

Protocol parameters cannot have their own bounds. Protocol requirements have
pass bodies and cannot declare method-local parameters. A generic implementation
method does not yet satisfy a monomorphic requirement. Protocols are constraints,
not value types; dynamic protocol values and inheritance are not implemented.
Protocol arguments exclude unit, references, generators, and closure environments.
