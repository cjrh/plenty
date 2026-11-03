# Boxed protocol objects

**Status: proposed; not implemented.** This proposes explicit owned protocol
objects, written `Box[P]`, for collections and interfaces whose concrete
implementations may differ. The [protocol reference](../design/language/07-generics/protocols.md)
and [box reference](../design/facilities/01-builtins/boxes.md) describe current
behavior. Implementation remains a candidate under **B29** in the
[backlog](../backlog.md); writing this proposal does not schedule it.

The investigation used local `main` at `bb97038b` on 2026-11-03. It found a
credible implementation path through the existing boxed-callable machinery,
not a working implementation of boxed protocols. Read the
[public contract](boxed-protocols/contract.md),
[implementation design](boxed-protocols/implementation.md),
[research evidence and runnable probes](boxed-protocols/evidence.md), and
[qualification criteria](boxed-protocols/validation.md) for the details.

## Intended behavior

A program should be able to store instances of different classes and enums,
iterate over them, and call the methods declared by a common protocol without
matching or casting. Every method on an object must reach the same underlying
instance, including changes made through mutable methods.

Hypothetical complete program; `Box[MyProtocol]` and its constructor do not
compile today:

```text
protocol MyProtocol:
    def hey(self) -> i64:
        pass

class Counter:
    value: i64

    def hey(self) -> i64:
        self.value

enum Message:
    Ready
    Count(i64)

    def hey(self) -> i64:
        match self:
            case Message.Ready:
                0
            case Message.Count(value):
                *value

def main() -> Result[(), AllocError | IoError]:
    mut items: list[Box[MyProtocol]] = []?
    items.append(Box[MyProtocol](Counter(10))?)?
    items.append(Box[MyProtocol](Message.Count(20))?)?
    for item in &items:
        print(item.hey())?
    Ok(())
```

Expected output is `10` then `20`. The list stores one common owner type;
each owner retains its concrete payload and the methods needed to operate on
it. The collection does not need a declaration listing every implementation.

Dictionary values work the same way, with ordinary supported keys:

```text
items: dict[str, Box[MyProtocol]] = {
    "counter": Box[MyProtocol](Counter(10))?,
    "message": Box[MyProtocol](Message.Ready)?,
}?
for name, item in items.items():
    print(name, item.hey())?
```

This does not extend dictionary keys or set elements. See the current
[heterogeneous collection lessons](../tutorial/heterogeneous-collections/index.md)
for implemented enum and anonymous-sum alternatives.

## Why `Box` is explicit

The proposed spelling is **`list[Box[MyProtocol]]`, not `list[MyProtocol]`**.
`MyProtocol` remains a compile-time contract rather than a standalone stored
value type. The `Box` boundary supplies owned storage for an unknown concrete
layout, while `Box[MyProtocol](value)?` exposes allocation and its failure.

This is a deliberate language choice, not a mathematical requirement. An
unboxed-looking protocol value could be defined as an implicitly boxed owner,
a borrowed view, or a fixed-capacity inline container. Each would need a
different storage and lifetime contract. None is implied by this proposal.

Ordinary `Box(value)?` continues to produce `Box[T]` for the concrete `T`.
An annotation or collection insertion does not silently allocate or erase it.
The explicit protocol argument selects erasure. Direct `P`, `list[P]`, and
bare `&P` remain unsupported as value types in the initial design; functions
can accept `&Box[P]` or an existing generic `T: P` bound.

## Recommended scope

Support one protocol per box, shared and mutable receiver methods, fixed
protocol type arguments, owned storage in ordinary aggregates, and existing
generic consumers of that same protocol. Require every method to meet the
[boxing eligibility rules](boxed-protocols/contract.md#eligible-protocols).
Keep structural conformance and explicit allocation; introduce neither
inheritance nor registration of implementations.

The initial contract excludes method-local generics, implementing-type
occurrences outside the receiver, consuming receivers, returned borrows,
downcasting, protocol-to-protocol conversions, copying, equality, and native
worker/channel transfer. These restrictions do not reduce what protocols can
express in today's static generic calls.

## Alternatives and tradeoffs

| Representation | Best fit | Consequence |
| --- | --- | --- |
| `list[Item]` with an enum wrapper | A known, finite set of alternatives | Exhaustive matching and inline payload storage; adding a variant changes the wrapper. |
| `list[A \| B]` | A small explicit set, dispatched by matching | No wrapper declaration; arbitrary user protocols are not forwarded today. |
| `list[Box[P]]` | Implementations supplied independently of the collection | Explicit per-object allocation and indirect method calls; consumers know only `P`. |
| `list[BoxedCallable[[], i64]]` | One reusable operation per object | Works today with an owned closure adapter; it does not provide several named methods over one mutable instance. |

An automatically generated enum of every implementation in the program would
make the set depend on global discovery. Its layout, matching, compilation and
module behavior need a separate design; it is not the mechanism proposed here.

A separate name such as `BoxedProtocol[P]`, or a `dyn P` spelling inside `Box`,
could emphasize erasure. Prefer the requested `Box[P]` spelling, provided the
compiler and documentation clearly distinguish its restrictions from concrete
`Box[T]`. In particular, an erased payload cannot be extracted by value.

## Feasibility assessment

This is a moderate, cross-cutting compiler feature. Type checking, generated
adapters, indirect calls and concrete destruction already have useful
precedents. Existing list/dictionary storage should accept a fixed owner
representation without a new collection implementation.

The difficult work is preserving borrowing and effect guarantees after the
concrete type disappears from the caller, not allocating memory or selecting
a function pointer. The estimate is architectural judgment, not a calendar
estimate or a measured prototype result.

This is independent of [protocol method bodies](protocol-method-bodies.md).
Defaults, final methods, private-hook grants, and abstract checking of supplied
bodies need their own implementation. Boxed objects can initially dispatch the
accessible inherent methods required by protocols with `pass` bodies.
