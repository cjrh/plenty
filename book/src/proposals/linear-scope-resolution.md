# Synchronous resolution scopes

Status: **Not implemented; separate proposal depending on [linear obligations](linear-obligations.md).**
Issue #149, [B72](../backlog.md). Current
[resource scopes](../design/language/08-errors/03-resource-scopes.md) and
[destruction](../design/language/05-ownership/06-destruction.md) do not provide
the consuming-finalizer contract proposed here. All examples are pseudocode.

## Why a separate extension

Manual transfer or resolution is a complete synchronous linearity rule, but a
transaction around several fallible calls needs repetitive handling of every
early exit. A manager can take responsibility, lend access to the body, and run
an authorized resolver before the scope returns. This is cleanup policy on top
of ownership; neither a new effect language nor async cleanup is necessary.

Python context managers illustrate the related control-flow guarantee:
following successful `__enter__`, ordinary body exits invoke `__exit__`. That
mechanism alone neither checks non-abandonment across calls/storage nor requires
every resource to enter a context. It does not statically stop escaped aliases,
promise cleanup success, or survive abrupt process termination. Plenty already
adds borrowing restrictions; linearity would additionally check the manager's
own responsibility.

## Proposed narrow contract

A resolution scope owns a hidden linear manager and registers a **consuming**
exit operation before lending body access. On each ordinary exit, body-local
owners finish first, borrowed access ends, then the manager is consumed exactly
once. The operation must resolve or transfer its payload on every returning
path. Its normal receiver is owned, unlike today's borrowed `__exit__`.

```text
with resolving(tx, release_local_transaction) as access:
    insert(access, record)?
    update(access, other_record)?
# every ordinary body exit consumes the manager and resolves tx
```

The illustrative resolver is explicitly a local-release policy that may attempt
rollback but promises no remote success. There is **no implicit commit-on-success
policy**. A library can offer a clearly named rollback/local-release guard;
the language should not guess the business meaning of normal scope exit.

Acquisition and registration form one ownership contract: failure returns the
unregistered owner, or occurs before transfer. Once registered, even a failed
entry operation must return the owner or run the resolver. Do not copy today's
successful-entry rule in a way that leaks an already-owned obligation. Nested
scopes finish in reverse order. If a later manager acquisition fails, earlier
registered managers still resolve, and the failed acquisition's returned owner
must itself be handled.

The body receives a loan bounded by the manager. It cannot move the manager,
replace its unresolved payload, retain the loan past exit, or return references
to owned manager storage. Return values may be prepared before cleanup only if
they obey these lifetime rules. A body-local linear owner is not automatically
the manager's responsibility unless explicitly transferred into it.

An explicit consuming finish inside the body needs a checked manager-state
transition that proves the exit operation has nothing left to resolve. The
first design may reject this and require manual ownership for commit/retry
flows. Assuming that a boolean `finished` flag makes a linear field disappear
would undermine the core rule. Similarly, borrowed `with &mut existing` cannot
consume the caller's owner; adapting it requires a separate state contract.

## Failure determines the available surface

The narrow automatic-exit form requires a synchronous resolver that returns
unit after reaching an allowed terminal *local* outcome. It may block, and it
may internally retry. If the only correct resolver returns a still-live owner
on failure, this form is unavailable. Swallowing that failure is not resolution.

Today's unit-returning `__exit__` is sufficient to express an infallible
side effect, but not sufficient to prove that a borrowed manager's linear
payload was consumed. A dedicated consuming-finalizer lowering (or a later
verified state-changing exit contract) is necessary. The hidden owner must not
subsequently reach ordinary drop with a live seal. This extension does not
change all existing managers or reinterpret existing `__exit__` hooks.

Fallible finishing should initially remain explicit. A future combinator can
make error precedence and retained ownership visible instead of silently
overriding a pending return or propagating an exit error through today's `with`:

```text
resolve_scope(owner, body, resolver) -> ScopeOutcome[BodyValue, BodyError, R]
ScopeOutcome = Resolved(Result[BodyValue, BodyError], ResolutionReport)
             | Unresolved(Result[BodyValue, BodyError], R, ResolveError)
```

`Unresolved` retains the owner `R`; the whole outcome is non-discardable. Both
body failure and resolution failure survive. If the body value/error is itself
linear, it remains owed in either branch. If allocation is needed to store an
outcome, its failure contract must retain these values too. The body borrows
the owner and returns a result to the combinator; it cannot return a borrowed
view of that owner. This sketch does not introduce nonlocal callback returns.

This explicit outcome is the recommended direction for recoverable resolution
failure, but callback syntax and a library implementation are open. A future
fallible `with` syntax would need equally explicit rules for simultaneous errors,
prepared return values, nested manager failures, and unresolved owners. It is
not a prerequisite for core linearity or the narrow unit-exit form.

## Destructors, effects, and execution limits

An ordinary `__del__` remains fallback memory/resource cleanup. It cannot be
used to launder a linear field into discardable storage. After terminal
resolution, remaining ordinary fields may be destroyed in their existing order.
This avoids requiring recoverable errors, suspension, or a stronger destructor
model merely to express meaningful finishing.

Resolver calls, including compiler-inserted ones, belong in the existing
reachable-effect graph. A resolver forbidden on a worker makes that scope
ineligible there; see the [integration example](linear-integration.md).
Opaque higher-order resolver effects remain subject to current certification
limits. There is no generic “cleanup is always allowed” exception.

No bounded completion is promised: scope exit can wait indefinitely. An
immediate timeout return and completed cleanup cannot both be unconditional.
Transferring responsibility to a longer-lived scope is possible only if it owns
all needed resources and retains no loans whose owners have already returned.
Async cleanup expressibility and the runtime obligation to drive it need a
distinct proposal; neither Python `async with` nor a hypothetical async
destructor alone supplies that obligation.

## Conformance and unresolved choices

Test normal return, `?`, break, continue, nested acquisition failure, and explicit
body failure with a counted resolver: exactly one permitted resolution for each
registered owner, body loans ended first, and unchanged cleanup order for
ordinary values. Reject escaping loans, destroying an unentered manager,
calling an empty resolver, consuming through a borrowed manager, and a resolver
returning while still owning its payload. Verify worker diagnostics include
inserted exit calls and their origin scopes.

The consuming-exit declaration surface, safe explicit finish within a scope,
and the fallible outcome combinator remain open decisions. The proposed
defaults are explicit policy, no implicit commit, no ignored recoverable
resolution error, and no async requirement.
