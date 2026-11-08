# Structural protocols

```plenty fragment
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
protocol. For a concrete implementation, a protocol parameter absent from every
requirement still needs explicit arguments. A `Box[P[Args...]]` supplies its
fixed arguments directly. Inference follows known signatures without searching for a
type or implementation.

Visibility is checked from the bound's defining module before method signatures
supply evidence. Public contracts cannot expose private protocols, requirement
types, or concrete arguments. Importing a protocol adds no methods to a type.

A requirement may declare its own type parameters, each unconstrained or
constrained by one protocol. An implementation then declares a generic method
with as many parameters, the same constraints, and the same signature once its
parameter names are replaced by the requirement's; the names may differ.
Calls through the bound instantiate the implementation for each argument type:

```plenty
protocol Sized:
    def size(self) -> i64:
        pass

protocol Sink:
    def put[S: Sized](self: &mut Sink, item: &S) -> Result[i64, OverflowError[i64]]:
        pass

class Crate:
    width: i64

    def size(self) -> i64:
        self.width

enum Parcel:
    Letter
    Packet(i64)

    def size(self) -> i64:
        match self:
            case Parcel.Letter:
                1
            case Parcel.Packet(width):
                *width

class Total:
    sum: i64

    def put[T: Sized](self: &mut Total, item: &T) -> Result[i64, OverflowError[i64]]:
        self.sum += item.size()
        Ok(self.sum)

def feed[K: Sink, S: Sized](sink: &mut K, item: &S) -> Result[i64, OverflowError[i64]]:
    sink.put(item)

def main() -> Result[(), IoError | OverflowError[i64]]:
    mut total = Total(0)
    feed(&mut total, &Crate(3))?
    feed(&mut total, &Parcel.Letter)?
    print(feed(&mut total, &Parcel.Packet(4))?)?
    Ok(())
```
```output
8
```

`Crate` and `Parcel` both satisfy `Sized`, so `feed` accepts either. Use an
enum to model a sum type, a value that is one of several variants, and a class
to model a product type, a record that holds all of its fields. Either can meet
the same protocol.

A generic method cannot satisfy a requirement without type parameters, and a
non-generic method cannot satisfy one with them.

Protocols constrain class and enum arguments, the built-in `bytes`, and an
explicit [protocol box](boxed-protocols.md) exposing the identical protocol.
A protocol alone is not a runtime value type. `bytes` has one
method, `len(self) -> i64`; other built-in types have none to offer. Protocol
parameters cannot have their own bounds; protocol arguments can be unit but exclude
references, generators, and closure environments. Fields, inheritance, multiple
bounds, defaults, associated types, and lifecycle requirements are unsupported.

Generic bodies are checked per concrete instance. Requirements express the
minimum interface rather than a separately checked abstract body. `IntType`,
`FloatType`, [`CopyType`](index.md#copytype), and [`KeyType`](index.md#keytype)
remain separate built-in constraints.

## Anonymous sums

An [anonymous sum](../03-types/anonymous-sums.md) satisfies a protocol bound
when every normalized member satisfies the protocol and every requirement can
be forwarded. In the generic body, each requirement call matches a shared
borrow of the sum and calls the active member's method once, passing the other
arguments unchanged. Nothing is allocated or erased, and member order and
aliases do not matter:

```plenty
protocol Labelled:
    def label(self) -> str:
        pass

class A:
    pub def label(self) -> str:
        return "a"

enum B:
    One
    pub def label(self) -> str:
        return "b"

def label[T: Labelled](value: &T) -> str:
    value.label()

def main() -> Result[(), IoError]:
    first: A | B = A()
    second: B | A = B.One
    print(label(&first), label(&second))?
    Ok(())
```
```output
a b
```

A requirement can be forwarded when:

- its receiver is `self` or `self: &P`; a mutable or consuming receiver is
  rejected;
- the implementing type `P` appears only as the receiver, so `-> P`,
  `other: &P` and `Box[P]` are rejected;
- it returns no reference or view into the member;
- each method type parameter appears in a forwarded argument, so the call
  determines it. A bound on such a parameter may name the protocol itself;
  the sum can then satisfy it.

A protocol with type parameters does not forward. The diagnostic names the
first requirement that cannot be forwarded and why. A member missing a
method, or with a mismatched signature, rejects the whole sum regardless of
which member is active. Member methods must be visible both from the
protocol's declaration, where the dispatcher is created, and from the bound.

Only the bound forwards. A direct call such as `second.label()` outside a
body bounded by the protocol is an error, even when every member has the
method and some protocol requires it: match on the sum instead. The one
exception is `sum.format(output)`, which dispatches through
[`std.fmt.Formattable`](../../stdlib/report.md).

## Design history

The [original protocol design](../../../proposals/completed/protocols-generics-and-context-managers.md)
records the shipped foundation. [Protocol method bodies](../../../proposals/protocol-method-bodies.md)
propose defaults, final methods, and explicit private-hook grants; those
extensions are not implemented. For an existing-language solution, see
[library-owned operations](../../../proposals/completed/library-owned-operations.md).
The [boxed protocol design](../../../proposals/completed/boxed-protocols.md) records the
research and rationale for implemented [explicit erased owners](boxed-protocols.md).
