# Checked defaults: implementation and migration

> Companion to [checked arithmetic by default](../checked-by-default.md).
> **Not implemented.** This is a feasibility assessment and proposed acceptance
> contract, not benchmark evidence. Scheduling stays in **B14** in the
> [backlog](../../backlog.md).

## Reuse and limits

Source inspection at local `main` revision `96f6fc42` found useful existing
machinery in `src/frontend/checked.rs`, `src/codegen/checked.rs`, and
`src/numeric.rs`: a stack of checked scopes, per-operation error collection,
cleanup paths, and cold exits producing a typed error. Scope records already
distinguish function forwarding from arithmetic checking. The parser already
recognizes contextual `checked:` suites in value positions.

Adding an explicit trapping mode and retaining nested `checked:` should be a
contained extension of that machinery. Represent the nearest arithmetic mode
explicitly; do not merely search past an unchecked scope for an outer checked
target. Separate the arithmetic mode from callback error forwarding, which is
already a distinct concern. Function failures must use the declared public
`Result` ABI, not blindly reuse the extra private wrapper used for checked
callbacks. Success returns, `?`, early exits, cleanup and tail calls need auditing.

The whole change is **not** a small default toggle. Most arithmetic-bearing
functions currently declare plain returns, and generic/protocol bodies,
callbacks, generators, runtime helpers and std signatures depend on that.
No implementation or timing experiment has been performed for this proposal.

## Functions and callbacks

A named function keeps its own declared behavior regardless of its caller's
mode. Ordinary anonymous functions also get their own default function error
boundary: their declared return type must cover their arithmetic, just as a named
function's does. Do not turn every closure created in an implicitly checked
function into a scope-bound closure with a hidden error channel. That would
prevent ordinary escaping callbacks and spread private effects across APIs.

Preserve the existing special rule for closures lexically created inside an
**explicit** `checked:` block: their arithmetic can exit that block through the
private checked channel, and their existing escape and erasure restrictions
remain. This honors the lexical callback decisions recorded in issue #614 and
the [current reference](../../design/language/03-types/numbers.md#checked-blocks).
An ordinary closure returning `Result` and an explicit checked closure carrying
a private arithmetic error channel remain distinct.

An `unchecked:` suite masks that explicit lexical inheritance at a closure's
definition site. Such a newly defined closure starts with the ordinary default
function boundary; trapping arithmetic in its body requires its own
`unchecked:`. This avoids a source opt-out silently creating an escaping
trapping function. A closure previously created in `checked:` keeps its checked
channel when called from a nested unchecked region: forward the failure to its
active checked boundary, while arithmetic written in the unchecked region
traps. Never discard a callback error channel because the caller opted out.

These rules add boundary cases, but reuse existing explicit checked closures
without making them the representation of all functions. Removing lexical
checked callbacks instead would be a separate source-breaking design reversal;
this proposal does not require it.

Ordinary callbacks whose return becomes `Result[T, E]` no longer satisfy a
`Callable[..., T]` contract. Their consumers must accept and handle the fallible
result, use an existing explicit checked callback region when suitable, or have
a justified opt-out in the callback. Audit pipelines, sorting, generic helpers,
worker jobs and boxed callables rather than blanket-wrapping callbacks in
`unchecked:`. Worker boundaries still cannot carry scope-bound checked closures.

## Special boundaries and library code

- **Generators:** today's `Generator[T]` cannot return a function-level `Err`
  from its suspended body. Reject unhandled recoverable arithmetic there with a
  targeted diagnostic; locally handle a `checked:` result or use a deliberate
  `unchecked:`. A fallible generator protocol is not implied by this proposal.
- **Destructors and fixed protocol returns:** where a signature must remain
  infallible, recover locally or explicitly opt out. Do not silently exempt all
  such bodies from the default.
- **FFI and exports:** preserve their ABI contracts. A wrapper must handle errors
  before a fixed C boundary; checked defaults do not invent a cross-language
  `Result` ABI. Native implementations need their own documented arithmetic
  audit because they are not Plenty source suites.
- **Generic functions and recursion:** validate against declared returns and
  bounds, with specialization diagnostics where needed. Do not discover public
  signatures by a call-graph fixed point or change effects with inlining.
- **Std and builtins:** apply the same default to Plenty implementations. Audit
  builtin integer reductions such as `sum`, which currently trap regardless of
  surrounding `checked:`. Prefer a public fallible contract where arithmetic can
  fail; retain a trap only as an explicit, documented choice. Fallible user
  operator hooks remain ordinary calls returning `Result`, not implicit effects.
- **Internal compiler/runtime arithmetic:** compiler implementation language
  operations do not acquire Plenty source semantics. Generated operations that
  implement a public primitive must nevertheless honor that primitive's contract.

These are compatibility obligations, not reasons to pretend all current
library arithmetic becomes recoverable just by changing user-function lowering.
Any public std signature changes need a std-conventions review and regenerated
signature index as part of implementation.

## Source compatibility and teaching

This deliberately breaks arithmetic-bearing plain-return functions. A useful
migration diagnostic offers three meaningful options: declare and propagate the
precise errors, handle a local checked result, or mark a narrowly justified
trapping region. Do not automatically insert unchecked blocks throughout the
repository to make the build pass: that would defeat the chosen default.

Existing explicit checked expressions generally remain useful. Arithmetic after
handling their errors now also needs scrutiny: a recovery calculation can itself
fail. Existing checked closures retain their restrictions, while newly fallible
ordinary closures require callback contract changes. Reserve the contextual
`unchecked:` suite without reserving the identifier everywhere.

Teach simple arithmetic in a fallible entry point, with a short explanation of
`Result` and `Ok(())`, then introduce local recovery and finally deliberate
opt-outs. Revisit application examples, counters, loop updates, sizes, parsing,
std implementations and numeric helpers. Keep tutorial/reference examples on
current behavior until implementation lands; proposal examples use `text` fences.

Choose and document the compatibility release boundary before adoption. The
[edition proposal](../language-editions.md) is exploratory, so this design cannot
assume editions exist. A compiler-version break is sufficient for an initial
migration; mixed-semantics dependencies would require an explicit compatibility
mechanism, never a caller-selected runtime mode.

## Validation contract

Acceptance needs semantic tests for function propagation, precise unions,
missing-error diagnostics, locally handled errors, all nesting directions,
value/statement suites, and ordinary calls returning `Result` inside unchecked
code. Exercise every integer width, division proofs, `MIN // -1`, `MIN % -1`,
negative powers, constant operands, compound assignments and forwarded newtypes.

Ownership tests must cover resources and temporaries on failure, partial
mutation, skipped writes, `?`, explicit returns, loop exits and tail recursion.
Callback tests must distinguish ordinary fallible closures from explicit checked
closures, including stored callbacks, generic borrowers, nested unchecked calls,
escape rejection, generators and worker boundaries. Inlining and optimization
must preserve the lexical contract. Trapping tests need child processes and must
verify termination in both debug and optimized builds.

Compare today's unadorned trapping arithmetic, today's explicit checked block,
proposed implicit checked function, and proposed unchecked block on equivalent
workloads. Inspect emitted IR/object code as well as timings: count guards,
result construction, cleanup and return ABI differences. Measure tight loops,
call-heavy kernels and realistic std consumers on success and failure paths.
Verify the no-allocation arithmetic contract and report compiler/build-time and
code-size costs. No speedup claim is justified before these measurements.

The [original checked-arithmetic design](../completed/checked-arithmetic.md)
records why local PR #678, which returned a `Result` from every operator, was
abandoned. Preserve its lesson: use plain values between operators and a shared
failure exit, rather than reviving per-operator wrapping and unwrapping.

Adoption also requires migrated executable tutorial/reference examples, updated
arithmetic and trap documentation, the std signature index, the full relevant
compiler/runtime tests and the complete docs CI checks. The proposal itself
changes none of those implemented contracts.
