# Structural protocols

```python
protocol Readable[T]:
    def read(self) -> T:
        pass

def read[T, R: Readable[T]](source: &R) -> T:
    source.read()
```

A protocol declares required ordinary method signatures with `pass` bodies.
A bound accepts a class whose accessible inherent methods match every requirement,
even requirements unused by the generic body. Receiver borrowing, arity,
parameter types, and return type must match exactly; parameter names may differ.
There is no inheritance, registration, or import-driven method activation.

Bare `self` means a shared receiver. Mutable requirements use
`self: &mut Readable[T]` or `self: &mut Readable`. Within requirements the protocol's
own name denotes the implementing class, including in returned references.

Protocol arguments substitute into all requirements. Once an input determines
the implementing class, its accessible concrete signatures supply inference
evidence for other parameters, including nested types and returns. Evidence must
agree with ordinary arguments and callable constraints. A protocol parameter
absent from every requirement still needs explicit arguments. Inference follows
known signatures without searching for a class or implementation.

Visibility is checked from the bound's defining module before method signatures
supply evidence. Public contracts cannot expose private protocols, requirement
types, or concrete arguments. Importing a protocol adds no methods to a class.

Protocols constrain class arguments rather than defining runtime value types or
existential containers. Protocol parameters cannot have their own bounds;
protocol arguments exclude unit, references, generators, and closure environments.
Fields, inheritance, multiple bounds, defaults, associated types, lifecycle
requirements, and method-local requirement parameters are unsupported. A generic
implementation method cannot satisfy a monomorphic requirement.

Generic bodies are checked per concrete instance. Requirements express the
minimum interface rather than a separately checked abstract body. `IntType`
remains a separate built-in numeric constraint.
