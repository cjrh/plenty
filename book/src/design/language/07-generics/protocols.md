# Structural protocols

```python
protocol Readable[T]:
    def read(self) -> T:
        pass

def read[T, R: Readable[T]](source: &R) -> T:
    source.read()
```

A protocol declares required ordinary method signatures with `pass` bodies.
A bound accepts a class or enum whose accessible inherent methods match every requirement,
even requirements unused by the generic body. Receiver borrowing, arity,
parameter types, and return type must match exactly; parameter names may differ.
There is no inheritance, registration, or import-driven method activation.

Bare `self` means a shared receiver. An owned requirement uses `self: Readable[T]`
or `self: Readable`; its implementation must also take its receiver by value.
Owned, shared and mutable receivers are distinct requirements. Mutable requirements use
`self: &mut Readable[T]` or `self: &mut Readable`. Within requirements the protocol's
own name denotes the implementing type, including in returned references.

Protocol arguments substitute into all requirements. Once an input determines
the implementing type, its accessible concrete signatures supply inference
evidence for other parameters, including nested types and returns. Evidence must
agree with ordinary arguments and callable constraints. A method type that no
arguments can fit, such as `-> ()` for `-> Result[(), E]`, does not satisfy the
protocol. A protocol parameter absent from every requirement still needs
explicit arguments. Inference follows known signatures without searching for a
type or implementation.

Visibility is checked from the bound's defining module before method signatures
supply evidence. Public contracts cannot expose private protocols, requirement
types, or concrete arguments. Importing a protocol adds no methods to a type.

A requirement may declare its own type parameters, each unconstrained or
constrained by one protocol. An implementation then declares a generic method
with as many parameters, the same constraints, and the same signature once its
parameter names are replaced by the requirement's; the names may differ.
Calls through the bound instantiate the implementation for each argument type:

```python
protocol Sized:
    def size(self) -> i64:
        pass

protocol Sink:
    def put[S: Sized](self: &mut Sink, item: &S) -> i64:
        pass

class Crate:
    width: i64

    def size(self) -> i64:
        self.width

class Total:
    sum: i64

    def put[T: Sized](self: &mut Total, item: &T) -> i64:
        self.sum += item.size()
        self.sum

def feed[K: Sink, S: Sized](sink: &mut K, item: &S) -> i64:
    sink.put(item)

def main() -> Result[(), IoError]:
    mut total = Total(0)
    feed(&mut total, &Crate(3))
    print(feed(&mut total, &Crate(4)))?
    Ok(())
```
```output
7
```

A generic method cannot satisfy a requirement without type parameters, and a
non-generic method cannot satisfy one with them. This is how `std.io.Writer`
accepts any [`std.bytes.ByteSource`](../../stdlib/bytes.md#byte-sources).

Protocols constrain class and enum arguments, and the built-in `bytes`, rather
than defining runtime value types or existential containers. `bytes` has one
method, `len(self) -> i64`; other built-in types have none to offer. Protocol
parameters cannot have their own bounds; protocol arguments exclude unit,
references, generators, and closure environments. Fields, inheritance, multiple
bounds, defaults, associated types, and lifecycle requirements are unsupported.

Generic bodies are checked per concrete instance. Requirements express the
minimum interface rather than a separately checked abstract body. `IntType` and
`FloatType` remain separate built-in numeric constraints.

[`std.report.Formattable`](../../stdlib/report.md) additionally accepts an
anonymous sum when every normalized member satisfies its contract. The compiler
dispatches to the active member through a shared borrow without allocation.
This is specific to that standard-library protocol: a user-defined protocol with
the same signature does not gain sum forwarding.

## Design history

The [original protocol design](../../../proposals/completed/protocols-generics-and-context-managers.md)
records the shipped foundation. [Protocol method bodies](../../../proposals/protocol-method-bodies.md)
propose defaults, final methods, and explicit private-hook grants; those
extensions are not implemented. For an existing-language solution, see
[library-owned operations](../../../proposals/library-owned-operations.md).
