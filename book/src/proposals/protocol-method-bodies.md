# Implemented protocol methods: feasibility sketch

**Status: exploration, not an accepted language change.** Investigated against
`d39e6ec` on 2026-10-20. The compiler still requires protocol methods to have
`pass` bodies. Future prioritization belongs to [B29](../backlog.md).

**Assessment:** shared bodies with explicitly selected protocol calls are a
small-to-medium frontend feature. Convenient calls through a protocol bound
are a medium feature. Automatically adding methods to every structurally
matching object, especially with private hooks, is a substantially larger
language-design change. None inherently requires dynamic dispatch, an object
wrapper, a new runtime representation, or a different borrow checker.

This investigates sharing implementations. The earlier
[library-owned operations assessment](library-owned-operations.md) addresses
preventing callers from bypassing an algorithm; it remains relevant, because
ordinary overridable defaults do not provide that guarantee.

The follow-up [final-method and visibility candidate](protocol-method-bodies/final-and-visibility.md)
lets each declaration choose whether its body is replaceable. It explores
rejecting a same-named inherent method at the structural boundary, with
[three short programs](protocol-method-bodies/examples.md). That final check
is a modest addition once provided-method selection exists; hidden external
hooks remain a separate access-control change.

## One example, three distinct choices

Hypothetical syntax, not executable Plenty:

```text
protocol DocumentStorage:
    def write(self: &mut DocumentStorage, document: &str) -> Result[(), IoError]:
        pass

    def save(self: &mut DocumentStorage, document: &str) -> Result[(), IoError]:
        validate(document)?
        self.write(document)?
        record_success()
        Ok(())
```

`FileStorage` and `DatabaseStorage` would supply `write`. The application wants
to call the shared `save`. Treating an AST body consisting solely of `pass` as
a requirement is straightforward; an intentional no-op implementation would
need an explicit unit-returning body. Comments do not change the AST body.

There are three independent questions:

1. **Sharing:** where may the application call the supplied `save`?
2. **Replacement:** may a concrete type supply its own `save` instead?
3. **Access:** may the application call `write` directly?

Implementing the first does not imply either answer to the other two.
In particular, the example above deliberately uses ordinary accessible hooks.
Cross-module private hooks need a separate access design.

## Options and relative cost

All new spellings below are illustrative.

| Design | Example call | Scope and assessment |
| --- | --- | --- |
| Existing bounded function | `save(&mut files, document)` | Works today. Sharing has no language implementation cost. |
| Protocol-owned operation | `DocumentStorage.save(&mut files, document)` | Small-to-medium. Explicitly selects the protocol body, lowers to a generic function, and adds no inherent method. |
| Default selected through a bound | `storage.save(document)` inside `S: DocumentStorage` code | Medium. Needs protocol-aware method resolution, missing-default handling, override rules, and receiver provenance through specialization. |
| Explicit adoption on a type | An illustrative `use DocumentStorage` declaration in `FileStorage` | Medium-to-large. A local place to opt into methods and diagnose conflicts, but introduces a new relationship beyond structural matching. |
| Automatic structural extension | `files.save(document)` wherever a matching protocol is visible | Large relative to the above. Needs candidate discovery, overlap rules, inference limits, and a decision about whether imports change available methods. |
| Final provided method | Reject a matching object with a same-named inherent method | Modest addition to provided-method selection: check the final-name restriction at conformance and consistently select the protocol body. |
| Private hooks across modules | Access controlled separately from structural matching | Separate substantial feature. Needs explicit authority, cross-module rules, and guarantees that survive every call path. |

These are engineering judgments from source inspection and experiments, not
calendar estimates or measurements of a completed implementation. A narrow
compiler spike would be small; a supported feature includes diagnostics,
modules, generic combinations, tooling, and regression coverage.

The protocol-owned operation is closest to ordinary generic-function sugar:

```text
# Conceptual lowering of DocumentStorage.save:
def __document_storage_save[S: DocumentStorage](storage: &mut S,
                                               document: &str) -> Result[(), IoError]:
    validate(document)?
    storage.write(document)?
    record_success()
    Ok(())
```

Here `DocumentStorage` checks only its required hooks; implemented operations
do not require same-named inherent methods. The generated name is an internal
detail. Calls must preserve the protocol definition's module visibility.

This design has deliberately fixed selection: `DocumentStorage.save(...)`
calls that body even when the object has an inherent `save`. It is a provided
operation, not a fallback override. It avoids global method searching and
preserves the existing rule that importing a protocol does not change an
object's inherent methods.

It does not, by itself, give the application the convenient `files.save(...)`
spelling. A bounded-call design could add that later, but must deliberately
choose whether it denotes the fixed operation or an overridable default.

## The important design traps

**Defaults are not a validation boundary.** If a concrete `save` replaces the
default, it can omit validation. Explicitly selecting a fixed protocol body
protects that call's algorithm; it does not prohibit calling a public `write`
or a separate concrete `save`. An owning wrapper remains the current-language
way to expose only the intended operation.

**Structural compatibility is not implementation selection.** Two protocols
may both require `write` but provide different `save` algorithms. A concrete
object can match both without selecting either. A qualified call is unambiguous.
For bound-based calls, the bound should identify the protocol; searching every
matching protocol makes imports and unrelated declarations affect lookup.

**Inherent overrides need exact rules.** If defaults are replaceable, decide
whether a same-named but incompatible method is an error or is ignored. Prefer
an error over silently choosing the default. Also specify calls between defaults:
does `save_twice` invoke an override of `save`, or its original protocol body?
Do not let these answers depend accidentally on specialization order.

**Checking bodies is a separate choice.** Existing generic bodies are checked
per concrete instance and may use methods not listed in their bound. Reusing
that machinery preserves today's behavior, but does not prove a protocol body
works for every conforming type. Abstract checking against just the contract
would be a broader B29 change. A syntactic scan for `self.foo` is insufficient:
aliases, helpers, and closures can carry the receiver.

**Inference needs real evidence.** Required concrete signatures can determine
protocol parameters. A supplied body cannot become circular evidence for its
own signature. If a protocol parameter occurs only in a provided operation,
require explicit arguments or other independent evidence. Avoid searching other
protocol defaults to manufacture conformance.

**Privacy is lexical today.** A library-defined shared body cannot access an
external class's private method. Automatically granting that access because a
public protocol happens to match would cross an existing encapsulation boundary
without the class author's consent. If pursued, access grants should identify
the protocol/member explicitly and be distinct from declaring conformance.
“Private to the class's module” also differs from “callable only by protocol
bodies,” which the original request might require.

## Precedents

Python's structural protocols provide a particularly relevant warning:
implicit structural matching does not supply default implementations; explicit
subclassing does. Its existing inheritance mechanism selects the shared code.
[Python typing specification](https://typing.python.org/en/latest/spec/protocol.html#explicitly-declaring-implementation).

Rust permits trait bodies as defaults that implementations may replace.
Individual trait items do not have independent visibility qualifiers.
[Rust Reference](https://doc.rust-lang.org/reference/items/traits.html).

Swift protocol extensions supply defaults; a conforming type's implementation
of a requirement takes precedence. Constrained extension overlap also requires
selection rules. This is useful precedent for both the feature and its semantic
cost, not a lookup model Plenty can copy unchanged.
[Swift language guide source](https://github.com/swiftlang/swift-book/blob/main/TSPL.docc/LanguageGuide/Protocols.md#providing-default-implementations).

## Suggested direction

The initial assessment favored **protocol-owned operations with explicit
selection** as the smallest extension. The follow-up candidate explores a more
expressive alternative: ordinary bodies are overridable defaults, `final`
bodies are fixed, and either can be selected through a bound or qualification.
It retains explicit structural matching without globally injecting methods.
The short programs make this candidate worth evaluating before settling on
qualified-only calls. Neither approach alone solves the private-hook scenario.

The [implementation map and probes](protocol-method-bodies/evidence.md) show
what already works and where the remaining compiler work lies. No new feature
is scheduled by this sketch.
