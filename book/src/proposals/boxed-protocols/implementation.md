# Boxed protocol implementation design

This supports the [boxed protocol proposal](../boxed-protocols.md). The source
map describes base `bb97038b`; representation and IR names below are suggestions,
not implemented APIs. Scheduling belongs to B29 in the [backlog](../../backlog.md).

## Reuse a concrete owner with an erased call interface

The existing `BoxedCallable` separates three facts that protocol objects also
need: a common public type, a concrete owned payload, and an adapter that knows
the payload's actual layout. For protocols, replace the single call adapter
with a static table containing one adapter per eligible method.

A candidate representation is:

```text
Box[P] owner
    |
    v
heap record
    ownership/allocation header
    concrete payload descriptor ------> layout, relocation and destruction
    protocol method-table pointer ----> method 0 adapter
    payload storage                    method 1 adapter
                                       ...
```

The public type has a fixed owner representation regardless of the payload's
size. Every table entry receives the same payload storage address. Adapters
recover the concrete receiver representation and invoke the selected inherent
method. Neither table lookup nor receiver adaptation allocates or copies the
payload. Keep the descriptor needed for destruction even if the protocol has
no cleanup-related method; lifecycle hooks are not protocol requirements.

`plenty-runtime/src/aggregates.rs` already has a `Boxed` header with a concrete
descriptor, a word currently called `padding`, and a payload slot at byte 32.
Boxed callable construction stores its adapter in that word. A protocol-table
address could occupy the analogous word for a distinct erased owner kind.
This suggests a compact implementation, not a frozen ABI or permission to
reinterpret a callable as a protocol object. Validate alignment, relocation,
destruction and target layouts before committing to offsets.

The existing allocator and descriptor-driven iterative box cleanup are the
preferred foundation. Do not replace them with a new reference-counting or
recursive destruction system. Explicit copying of an ordinary box must never
become an accidental way to copy a protocol object.

## Frontend and type identity

Introduce an explicit internal type, illustratively `Ty::BoxedProtocol`, whose
identity carries the resolved protocol and instantiated method signatures.
Keep the concrete payload type on construction/adapters, not in the erased
public identity. This avoids pretending that `P` has an ordinary inline size
or passing it through operations that assume every `Ty::Box(T)` can yield `T`.

Resolve `Box[P[Args...]]` through protocol-aware type resolution before the
ordinary concrete `Box` path tries to resolve its content as a value type.
Preserve imported aliases, visibility, generic substitution and canonical
identity. A protocol name alone must not start resolving as a stored value.
Explicit `Box[P](value)` construction also needs frontend handling; the current
`Box(value)` builtin merely infers its concrete content type.

Perform eligibility checking after resolving protocol arguments, while retaining
the implementing-type marker needed to detect forbidden non-receiver uses.
Then check the concrete input with the existing structural conformance rules.
Keep receiver mutability, exact signatures and module access checks; method
names alone do not establish conformance. Use positional method arguments
initially, following current protocol-bound call restrictions, rather than
inventing a keyword/default mapping from implementation parameter names.

Register adapters and the table for each canonical `(concrete T, protocol P,
protocol arguments)` combination. Repeated construction should share emitted
code and data. Cache emitted artifacts separately from access decisions:
visibility must still be checked in each appropriate source context.
Materialize every required concrete method, including methods of generic
classes/enums, so no table entry points to an uninstantiated definition.

For `T: P` instantiated with `Box[P]`, synthesize the exposed signatures with
the receiver replaced by a borrow of the owner and forward method calls through
the table. Inference should read the box's fixed protocol arguments, not inspect
hidden concrete types. Do not loosen generic matching or protocol identity to
make this case fit by accident.

## Typed IR and native lowering

Use explicit typed operations, illustratively `ProtocolBoxNew` and
`ProtocolBoxCall`, rather than smuggling an erased owner through concrete-box
operations. The IR checker must independently validate input ownership,
protocol identity, method index, receiver mutability, arguments, results and
adapter signatures. Construction consumes its concrete input and produces
the exact allocation-error result.

Calls load the table and statically selected method entry, then use the existing
indirect-call ABI machinery. The code generator may erase the receiver address
at the ABI boundary, as boxed callable lowering does, but the typed IR and loan
checker must retain the real shared/exclusive owner contract. Shared-call
`BoxedCallable` code is a precedent, not a drop-in mutable-call implementation.

Emit tables with native data/function relocations. Track adapter addresses and
their callees in reachability analysis, including executable pruning on Windows.
The compilation cache must include protocol identity, signatures, table order,
receiver mode and selected implementation bodies. A method reorder or signature
change must not reuse a caller expecting the old slot layout. Use deterministic
table order, initially declaration order; no stable external table ABI is
promised. Foreign export/import of these owners is outside the initial scope.

## Integration map

| Area | Existing locations | Required integration |
| --- | --- | --- |
| Types and construction | `src/frontend.rs`, `src/op.rs`, `src/boxed.rs` | Distinguish boxed protocols from concrete box extraction and type identity. |
| Protocols and generics | `src/frontend/protocols.rs`, `src/frontend/generics.rs` | Eligibility, conformance, fixed-argument inference, adapter specialization. |
| Method dispatch | `src/frontend/collections.rs`, `src/frontend/classes.rs` | Resolve methods of erased owners through fields, references, collection projections and temporaries. |
| Callback precedent | `src/frontend/callables.rs`, `src/codegen/closures.rs` | Generated adapters, fixed-signature indirect calls, fallible owner construction. |
| Independent verification | `src/op/check.rs` | Construction/call signature checks and receiver ownership checks. |
| Ownership and effects | `src/ownership.rs`, `src/ownership/calls.rs`, `src/ownership/futures.rs`, `src/frontend/provenance.rs` | Preserve loans and treat unknown indirect effects conservatively. |
| Runtime storage | `plenty-runtime/src/aggregates.rs`, `src/codegen/metadata.rs` | Concrete drop/relocation descriptor plus static method-table identity. |
| Reachability and artifacts | `src/codegen/reachability.rs`, `src/threading.rs`, codegen cache/dump paths | Retain adapters, serialize identities and reject uncertified transfer. |
| Shared owners | `src/frontend/shared.rs` | Prevent hidden payloads from bypassing cycle and executor/future ownership checks. |
| Modules and tooling | `src/frontend/modules.rs`, `src/frontend/ide.rs` | Public-interface validation, diagnostic blame and navigation to protocol requirements. |

All type walkers need an audit: storage barriers, contained owners, copying,
equality, formatting, generic substitution, references, checked effects,
serialization and FFI eligibility. An exhaustive type-enum match helps find
sites, but a wildcard arm can silently apply an unsafe default.

## Safety boundaries that erasure must preserve

Conformance proves signatures, not effects. A method may call back into user
code, allocate, wait, acquire a lock or access another owner. Existing indirect
call analysis is conservative; a protocol method table must not accidentally
look like an effect-free primitive. Audit lock-held waits, live future/task
obligations and reentrant arguments as well as ordinary borrow conflicts.

The existing `BoxedCallable` rejection at worker/channel boundaries is a useful
initial policy. Hiding a non-transferable object inside a list or a class must
not make it transferable. A future transferable protocol owner would need
certification at construction for payloads, reachable methods and destructors;
the presence of a matching protocol is not that certification.

Shared ownership has a second erasure hazard: a payload stored behind mutable
state can contain a share of its enclosing owner. Existing `Shared` traversal
already treats erased callbacks conservatively across such edges. Extend that
policy to protocol objects and audit hidden executors/futures too. Do not claim
that the ordinary box descriptor alone makes these static checks unnecessary.

Returned borrows are plausible later because Plenty ties a method's reference
result to its receiver. They still require forwarding the caller's owner loan
through an unknown implementation, including optional/fallible borrows and
temporary receivers. Reject every reference-carrying output in the initial
contract rather than copying only the direct-reference guard from boxed
callables. Borrow-carrying output must be recognized structurally.

## Relationship to protocol bodies

The [protocol-body proposal](../protocol-method-bodies.md) specifies defaults,
final operations and private-hook grants. It is not a prerequisite here. The
initial table contains concrete inherent implementations of `pass` requirements.

If both features ship, table construction should use the same resolved selection
rules as static bound calls. Default calls must honor overrides consistently;
final methods and access grants must retain their declared authority. Avoid a
second, independent method-selection algorithm in the backend. Abstract checking
of supplied bodies remains part of that proposal, not a capability established
by the single-method erasure probe.
