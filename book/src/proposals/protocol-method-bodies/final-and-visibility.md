# Final methods and hidden hooks

**Status: core design agreed; not implemented.** This refines the
[feasibility sketch](../protocol-method-bodies.md) following the proposed
`pub final def` spelling. [Short programs](examples.md) explore the resulting
programming model. Future prioritization remains in [B29](../../backlog.md).

## Separate implementation policy from call visibility

The agreed design has three implementation policies:

| Declaration | Requirement on a matching class |
| --- | --- |
| `def operation(...): pass` | Supply an inherent method matching the signature and visibility. |
| `def operation(...): <body>` | Supply an override matching the signature and visibility, or use the protocol body. |
| `final def operation(...): <body>` | Do not declare an inherent method of that name; use the protocol body. |

Visibility is independent: `pub` permits application calls through the bound;
a non-public hook is available to authorized protocol code. A final method
requires a body; `final` with only `pass` is contradictory and should be rejected.
An explicit unit-returning body can represent an implemented no-op.

`final` is a reasonable working keyword. It communicates that an implementation
cannot be replaced. `fixed` would emphasize ownership of the algorithm, but
does not resolve any additional semantic issue. Choose semantics first.

## Enforce final at the structural boundary

Matching `S: DocumentStorage` requires:

- Every required method exists with exactly matching signature and visibility,
  and any necessary access grant.
- Every supplied override matches the protocol signature and visibility exactly.
- No inherent method has the name of a final protocol method.

Check the entire contract even if the generic function never calls `save`.
Report the failed bound at its use, with notes identifying both the final
declaration and the conflicting concrete declaration.

The class remains valid on its own. It simply does not satisfy this protocol.
No global scan of all classes and protocols is needed. Explicit
`DocumentStorage.save(&mut value, ...)` enforces the same complete
contract rather than offer a way around conformance.

Name-only conflict detection follows the agreed rule: a same-named inherent
method fails the match, including an incompatible signature or private method.
There is no visibility-based exception to the final restriction. An internal
class edit can therefore break conformance. Field/method name collisions still
need an explicit rule before implementation.

Exact signature matching uses Plenty's existing structural signature rules,
including receiver borrowing, type parameters and return type; parameter names
need not be identical. Visibility is an additional equality requirement.

Once protocol-aware selection exists, this final-name check is a relatively
small addition to conformance checking. It does not require the broader access
grant machinery below. It also does not resolve how calls find protocol methods.

## A coherent initial lookup rule

Select methods through an explicit bound and through protocol qualification,
without automatically adding methods to concrete objects:

```text
def persist[S: DocumentStorage](storage: &mut S, text: &str):
    storage.save(text)       # Final protocol implementation.

DocumentStorage.save(&mut files, text)  # Same contract and selection.
files.save(text)             # No injected inherent method.
```

For an ordinary default, calls through the bound choose the compatible inherent
override if present. For a final method they always choose the protocol body.
Calls from a final body to a replaceable method follow the same rule. A
qualified call selects the same operation, including its override, to
avoid two meanings of the public API. Explicitly invoking the fallback body
would need a separate feature; it is outside the agreed design.

The earlier alternative where qualification always selects the original
protocol body was not selected. Fixed selection is expressed with `final`;
ordinary implemented methods provide overridable defaults.

A class author supplies hooks and optional overrides. Protocol code operates
on that existing object, subject to normal ownership and borrowing. It adds no
fields, ownership, allocation, or runtime wrapper. However, methods available
through a bound can differ from those available on the concrete object. This
needs teaching and completion support; the examples make the difference visible.

## What non-public means

The desired interface is:

```text
pub protocol DocumentStorage:
    def write(self: &mut DocumentStorage, document: &str) -> Result[(), IoError]:
        pass

    pub final def save(self: &mut DocumentStorage, document: &str) -> Result[(), IoError]:
        validate(document)?
        self.write(document)?
        record_success()
        Ok(())
```

`write` is the hidden customization hook, and `save` is the public entry point.
Three permissions must be distinguished:

1. Calling `write` through a `DocumentStorage` bound.
2. Calling the concrete object's inherent `write`.
3. Letting a protocol defined elsewhere call that concrete private method.

Visibility matching requires the concrete `write` to be non-public too; a
public implementation fails conformance. Existing module privacy remains:
code in the class's defining module can still call its private methods.
Class-only privacy and literal “only protocol bodies may call this method”
were considered and not selected.

For the third, structural matching alone should not authorize access to
unrelated private members. Otherwise application code could declare its own
protocol with a private `write` requirement and a public forwarding method,
then use matching to obtain the same access. This would defeat existing
module privacy.

The agreed class-side declaration is:

```text
pub class FileStorage:
    allow DocumentStorage.write

    def write(...):
        ...
```

This grants methods defined inside the named protocol access to this specific
hook. It grants no access to other private methods or fields. Any method defined
inside `DocumentStorage`, including a later addition, may call `write`.
Unrelated protocols with identical signatures receive no permission.

The grant is lexical, not inherited from the caller. A standalone helper in
the protocol's module does not receive it merely because a protocol method calls
the helper. Keep a hook-calling helper as a private implemented protocol method
when it needs the grant. Ordinary functions in the concrete class's module still
have their existing module access. If the class and protocol share a module,
that ordinary access remains available independently of the grant.

Broad grants to any protocol, module-wide switches, and a grant to all of one
protocol's requirements were considered and not selected. Member-specific
permission makes expansion deliberate. The class knows the protocol's identity;
putting the contract in a module independent of concrete backends avoids a cycle
in the usual library/backend/application arrangement.

No access scheme prevents an implementation from exposing a separate operation
or performing its own I/O; the guarantee concerns the declared interface.

## Remaining specification work

The core behavior above is settled. This is not yet a complete implementation
specification or approval to implement the feature.

**Compatibility:** today's protocol requirements use bare `def` and remain
callable from bounded generic functions. Reinterpreting every bare `def` as a
hidden hook requires a deliberate stdlib, example, and tutorial migration.
Adding a final member can also invalidate existing structural matches.

**Body checking:** retain per-instance generic checking initially, or require
provided bodies to work using only the protocol contract? The latter is broader
work. This is the main remaining user-facing design choice. For example, should
a provided body calling an undeclared `self.extra()` fail at its definition, or
only when instantiated for an object lacking `extra`? Preserve definition-site
access either way, and never let per-instance checking bypass visibility.

**Inference and overlap:** identify the protocol from the bound, not from
the concrete receiver alone. Two bounds on different parameters can refer to
the same concrete type and select different provided bodies. Combined bounds
remain outside this design; qualification selects a protocol without global
discovery. Rules for underdetermined type arguments and receiver aliases need
precise specification and regression cases.

**Access and collision edge cases:** specify validation of an `allow` target,
access in closures lexically nested inside protocol methods, and fields sharing
a provided method name. These should be resolved explicitly rather than left to
accidental behavior of generated functions.

**State and validity:** final freezes an algorithm, not its results. Hook
contracts still define partial failure, side effects, and reentrancy. Persistent
accounting/state must live in the object or a wrapper; a protocol supplies no
new storage.

`final` remains the working keyword. The semantic choice is settled; its final
spelling and diagnostic wording can be decided without reopening dispatch.
