# Protocol body implementation map and probes

This supports the [feasibility sketch](../protocol-method-bodies.md). Source
locations and results describe base `d39e6ec`, not a compiler with defaults.
The experiments use **handwritten generic-function lowerings**; they do not
implement new syntax, method selection, final-name checks, or private access grants.

## Existing compiler machinery

| Area | Current implementation | Implication |
| --- | --- | --- |
| Parsing | `src/frontend/protocols.rs`, `Parser::protocol_decl` | Uses the ordinary function parser and stores `Vec<Function>`, including bodies. An explicit guard rejects any body other than one `Pass`. Recognition is easy; removing the guard alone does not compile or register bodies. |
| Module resolution | `src/frontend/modules.rs`, protocol loop in `resolve` | Already walks each requirement through `Scope::function` and validates public signature types. Generated bodies must retain this definition-site namespace and access context, rather than borrowing the concrete class's privileges. |
| Conformance | `src/frontend/protocols.rs`, `check` | Every method is currently required. Provided operations need separation from requirements; overridable defaults additionally need missing-method fallback and compatible-override checks. Include generic requirements and the `bytes` special case. |
| Inference | `src/frontend/generics.rs`, `Engine::infer_bounds` | Reads concrete inherent signatures for protocol requirements. Must avoid trying to read a nonexistent inherent default or inferring its parameters from itself. |
| Specialization | `src/frontend/generics.rs`, `prepare` and `Engine::specialize` | Generic templates, type substitution, cached instances, and specialization budgets already exist. A protocol body can become a template parameterized by implementing type, protocol arguments, and method-local parameters. |
| Calls | `src/frontend/classes.rs`, `class_method` | Derives the concrete owner/method symbol, checks access, then selects concrete or generic inherent methods. No general protocol-default fallback exists. |
| Access | `src/frontend/modules.rs`, `check_member_at` | Private access compares the member's owner module with a source token. There is no protocol/member access-grant model. |
| Tooling | `src/frontend/ide.rs`, protocol declarations and `method_call` | Navigation needs the selected protocol body or override, not an invented concrete member. A feature must preserve source locations in generated templates and diagnostics. |

For explicit protocol operations, introduce distinct callable symbols, lower
their bodies into existing generic templates, and resolve the qualified
protocol call to that symbol. Keep conformance checks independent of compiling
the body. Ordinary static calls can then flow through the existing IR/backend.
There is no inherent need for protocol values, witness objects, vtables,
boxing, or additional heap allocations. These are architectural conclusions,
not benchmarked performance claims.

For bound-based syntax, more information must survive specialization.
`Engine::specialize` substitutes concrete types and clears function type
parameters. `protocol_instances` records a function-level flag used for
restrictions on named/default arguments; it is not a mapping from individual
receivers to protocol evidence.

Consider `left: &T` with `T: P` and `right: &U` with `U: Q`, instantiated with
the same concrete type. Choosing a default from the concrete type alone cannot
distinguish `left.operation()` from `right.operation()`. Aliases, returned
references, and captured receivers must preserve whichever selection semantics
the design promises. One possible approach is to resolve calls to explicit
protocol operations before erasing that information, but this needs more than
a textual receiver-name rewrite.

## Reproducible experiments

From a checkout containing the script, build its compiler and run:

```sh
cargo build --locked --bin plenty
python3 -B scripts/probes/protocol_methods.py target/debug/plenty
```

The script creates isolated temporary modules, disables the compiled-function
cache, and checks exact output or an expected diagnostic. It is a research
harness, not a new language conformance suite.

| Probe | Observed result |
| --- | --- |
| Library `save` and `save_twice`, external mutable backend, application in a third module | Runs; output `-1`, `3`, `10`, `-999`. Negative input is rejected before writing; nested shared calls update state; an inherent `save` remains a distinct operation returning `-999`. |
| Same backend with non-public `write` | Rejected as private to its defining module. |
| Application imports library-private validation helper | Rejected as private. |
| A protocol method with a real body | Rejected by the existing default-implementation guard. |
| Application calls `memory.save` when only the library function exists | Rejected as an unknown method. |
| Parameterized source protocol, concrete error inference, enum implementation | Runs; output `42`. |
| Required generic method with its own protocol bound, called twice by shared code | Runs; output `14`. |
| Shared generic function forwards a hook's returned reference | Runs; output `8`. |
| Mutation while that returned reference remains live | Rejected by borrowing checks. |
| Generic body calls a concrete method absent from its declared protocol | Runs; output `99`, confirming per-instance checking rather than abstract contract checking. |
| Final storage candidate lowered to a generic function | Runs; output `empty rejected`, `saved`, `1`; validation prevents the first write. |
| Final eligibility body with default and overridden score, lowered to two selected bodies | Runs; output `False`, `True`. Selection is handwritten, not inferred by a new compiler rule. |
| Two protocol operations on the same concrete sensor, including a receiver alias | Runs; output `20`, `68`; the lowering explicitly preserves the two selections. |

All **13 probes passed** with the compiler built from the exploration worktree.
Eight executed native programs and five checked expected compile failures.
No compiler/runtime code was changed. These probes establish reuse of existing
generics and ownership behavior; they do not establish that adding fallback
lookup is a small patch.

The last three correspond to the [candidate programs](examples.md). Their
handwritten lowering tests the algorithms and borrowing but cannot validate
automatic selection, alias provenance, or rejection of final-method conflicts.
Those remain compiler work. The cross-module private-hook sketch is not executed.

## Remaining validation if implemented

An implementation would need focused tests for qualified and bound-based
calls, imported aliases, two matching protocols, incompatible overrides,
calls between provided methods, recursion and specialization reuse/limits,
parameter inference without circular evidence, generic data and enum receivers,
`bytes`, mutable receiver reservations, returned loans and temporary receivers,
closures, diagnostics, and navigation. Changing body selection must also
invalidate the appropriate compiled artifacts.

New syntax should preserve existing rules for method-local parameters,
parameter defaults and positional calls unless those changes are separately
designed. An all-provided protocol and mutually recursive defaults need explicit
rules: accepting their structural shape does not prove their algorithm
terminates. Do not recursively compile bodies merely to decide conformance.

The body-lowering experiment did not test generated compiler symbols, actual
default dispatch, abstract body checking, access grants, compile-time overhead,
or binary-size changes. Full workspace tests and Clippy were not run for this
documentation/Python-harness change; they are required for a Rust implementation.
