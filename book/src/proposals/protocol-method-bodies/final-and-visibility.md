# Final methods and hidden hooks

**Status: candidate semantics, not implemented.** This refines the
[feasibility sketch](../protocol-method-bodies.md) following the proposed
`pub final def` spelling. [Short programs](examples.md) explore the resulting
programming model. Future prioritization remains in [B29](../../backlog.md).

## Separate implementation policy from call visibility

The candidate has three implementation policies:

| Declaration | Requirement on a matching class |
| --- | --- |
| `def operation(...): pass` | Supply an inherent method with the required signature. |
| `def operation(...): <body>` | Supply a compatible override, or use the protocol body. |
| `final def operation(...): <body>` | Do not declare an inherent method of that name; use the protocol body. |

Visibility is independent: `pub` permits application calls through the bound;
a non-public hook is available to authorized protocol code. A final method
requires a body; `final` with only `pass` is contradictory and should be rejected.
An explicit unit-returning body can represent an implemented no-op.

`final` is a reasonable working keyword. It communicates that an implementation
cannot be replaced. `fixed` would emphasize ownership of the algorithm, but
does not resolve any additional semantic issue. Choose semantics first.

## Enforce final at the structural boundary

For the strict candidate, matching `S: DocumentStorage` requires:

- Every required method exists with a compatible signature and allowed access.
- Every supplied override matches the protocol signature.
- No inherent method has the name of a final protocol method.

Check the entire contract even if the generic function never calls `save`.
Report the failed bound at its use, with notes identifying both the final
declaration and the conflicting concrete declaration.

The class remains valid on its own. It simply does not satisfy this protocol.
No global scan of all classes and protocols is needed. Explicit
`DocumentStorage.save(&mut value, ...)` should enforce the same complete
contract rather than offer a way around conformance.

Name-only conflict detection follows the user's proposed rule and catches an
incompatible signature too. Whether *private* same-named inherent methods also
conflict is an explicit decision: rejecting them avoids two meanings of the
method name, but makes an internal class edit break external conformance.
For now, the strict candidate rejects them; this is not a settled compatibility
policy. Also specify field/method name collisions before implementation.

Once protocol-aware selection exists, this final-name check is a relatively
small addition to conformance checking. It does not require the broader access
grant machinery below. It also does not resolve how calls find protocol methods.

## A coherent initial lookup rule

Explore methods through an explicit bound and through protocol qualification,
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
qualified call should select the same operation, including its override, to
avoid two meanings of the public API. Explicitly invoking the fallback body
would need a separate feature; it is not included in this candidate.

This differs from the earlier option where qualification always selects a fixed
protocol-owned body even in the presence of an inherent method. Keep those
alternatives distinct. The final candidate makes fixed selection explicit in
the declaration instead.

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

Yes: `write` is the hidden customization hook, and `save` is the public entry
point. But three permissions must be distinguished:

1. Calling `write` through a `DocumentStorage` bound.
2. Calling the concrete object's inherent `write`.
3. Letting a protocol defined elsewhere call that concrete private method.

Hiding the protocol member addresses only the first. Allowing a public
concrete `write` still permits a direct application call. Requiring a private
concrete method helps with the second, but today's privacy is module-based:
other code in that module may still call it. Literal “only protocol bodies”
requires a new, stricter access category.

For the third, structural matching alone should not authorize access to
unrelated private members. Otherwise application code could declare its own
protocol with a private `write` requirement and a public forwarding method,
then use matching to obtain the same access. This would defeat existing
module privacy.

Possible policies:

| Policy | Consequence |
| --- | --- |
| Hide only calls through the bound; require accessible concrete hooks | Least new access machinery, but does not prevent direct concrete calls. |
| Keep hooks private and put protocol and class in the same module | Ordinary lexical access works, but does not support independent external backends. |
| Let the class explicitly grant one protocol access to a hook | Supports external backends and retains encapsulation; new type-side authorization syntax/metadata is needed. |
| Wrapper and optional capability parameter | Existing-language solution described in the [earlier assessment](../library-owned-operations.md); no new access category. |

An illustrative class-side `allow DocumentStorage.write` declaration could
express the third policy without an `impl` block. It would grant access only
to the named hook/protocol, not to all private fields and methods, and would
not authorize other protocols with identical signatures. The spelling, exact
grant scope, and whether the implementing module can also call the hook remain
open. Do not make grants follow arbitrary helper calls or caller privileges.

If a public concrete hook must be rejected to enforce the desired privacy,
say so in the conformance rules. That is stronger than hiding a protocol view.
No access scheme prevents an implementation from exposing a separate operation
or performing its own I/O; the guarantee concerns the declared interface.

## Remaining decisions

**Compatibility:** today's protocol requirements use bare `def` and remain
callable from bounded generic functions. Reinterpreting every bare `def` as a
hidden hook requires a deliberate stdlib, example, and tutorial migration.
Adding a final member can also invalidate existing structural matches.

**Body checking:** retain per-instance generic checking initially, or require
provided bodies to work using only the protocol contract? The latter is broader
work. Preserve definition-site access either way.

**Inference and overlap:** identify the protocol from the bound, not from
the concrete receiver alone. Two bounds on different parameters can refer to
the same concrete type and select different provided bodies. Combined bounds
remain a separate question; qualification can resolve an explicitly selected
protocol without global discovery.

**State and validity:** final freezes an algorithm, not its results. Hook
contracts still define partial failure, side effects, and reentrancy. Persistent
accounting/state must live in the object or a wrapper; a protocol supplies no
new storage.

The promising candidate is therefore **bound-based provided methods, ordinary
overridable defaults, and final-name rejection**, with explicit qualification
as an escape from ambiguity. Hidden external hooks remain an additional design
decision, not something that the `final` modifier solves.
