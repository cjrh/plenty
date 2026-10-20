# Library-owned operations and backend hooks

**Status: Investigation complete.** Issue #145 recommends an API design using
existing composition, with capability parameters where needed; new protocol
visibility is deferred.
No new language feature or stdlib conversion is approved. The current contracts
remain [protocols](../design/language/07-generics/protocols.md),
[visibility](../design/language/02-source/visibility.md), and
[classes](../design/language/03-types/classes.md). Any future access extension
belongs to [B29](../backlog.md).

The later [protocol-body proposal](protocol-method-bodies.md) records agreed
default/final methods and member-specific private-hook grants. That design is
not implemented; the wrappers and capabilities here remain the existing-language
solution. Its [short programs](protocol-method-bodies/examples.md) contrast
the proposed programming model with this assessment's composition approach.

## Introduction

A library sometimes needs to let other packages customize one step of an
operation while keeping the surrounding algorithm under the library's control.
That algorithm may validate inputs, check state, acquire resources, and record
success or failure. The customizable step is an *implementation hook*: it does
the backend-specific work, but is not intended to be an independent application
entry point.

For example, a **library author** provides `Transport.send(message)`, which
checks the message-size limit and connection state, asks a backend to send the
message, and updates accounting after success. A **backend author** supplies
the TCP-specific `send_impl(message)` hook, or an in-memory version for tests.
An **application author** selects a backend and calls `transport.send(message)`.
These may be three different people working in three different packages.

Two mistakes should be prevented through that library interface. Application
code should not accidentally call `send_impl` directly and skip validation or
accounting. A backend's customization should not replace the surrounding
`send` algorithm and omit those checks either. Supplying the hook and calling
the hook are therefore different permissions. A similar situation arises when
`Store.save(record)` must validate a record and manage a transaction around a
backend-specific write: application code should use `save`, and each backend
should not have to reproduce the transaction algorithm.

C++ addresses this with the **non-virtual interface (NVI) pattern**: a public
non-virtual method owns the algorithm and calls a private virtual hook. A backend
class can override that hook, while application code using the base interface
cannot call it directly. The public algorithm is not overridden through that
interface. This is about separating customization from access, rather than
sealing an interface against external implementations.

Rust traits do not directly offer that combination: individual trait methods
cannot have independent private/public visibility, and a default method body
can be replaced by an implementation. Rust can express the intent using an
owning wrapper with a private backend field; a capability argument can further
restrict calls to an otherwise public hook. The
[precedents](library-owned-operations/precedents.md) explain and test these
workarounds. The question for Plenty is whether existing composition provides
clear enough guarantees, or whether protocols need a new way to permit external
hook implementations without exposing ordinary calls to those hooks.

The aim is to prevent accidental bypass through the library abstraction, not to
prove that a backend performs its work honestly. A backend can still expose its
own separate API, and validation, partial failure and transaction recovery need
explicit contracts regardless of method visibility.

## Decision and roles

Use an ordinary owning wrapper for the public algorithm. Keep its backend
field private. Use a public backend protocol when external implementations are
required. If normal application code must also be unable to invoke a hook on a
separately obtained backend, require a library-issued capability argument.
Defer new protocol access or implementation syntax: the tested examples do not
establish a missing guarantee that warrants that complexity.

This is the intent of C++'s non-virtual interface (NVI) pattern, not sealing.
[Precedents and probes](library-owned-operations/precedents.md) explain the
distinction and the Rust workarounds.

| Role | Transport responsibility | Permitted customization |
| --- | --- | --- |
| Library author | Owns `Transport[B].send`, validation, connection state and successful-send accounting | Changes the algorithm and hook contract |
| Backend author | Supplies `Memory.send_impl` or TCP-specific work, possibly in another package | Implements the hook; does not replace `Transport[B].send` |
| Application author | Constructs the selected backend/wrapper and calls `transport.send(message)` | Selects/configures a backend; cannot access the wrapper's private field |

One person may fill all three roles; their modules still have different access.
The guarantees concern use through this abstraction. A backend author can expose
another API on their concrete type or intentionally violate the hook contract.
Neither NVI nor a token proves honest transmission, storage, or accounting.

## Existing-language baseline

The [complete three-module example](library-owned-operations/examples.md)
executes with today's compiler. The library author moves an externally defined
public backend into `Transport[B]`, keeps `_backend`, `_limit`, `_open` and
`_sent` private, and implements one public inherent `send` method. A private
bounded helper checks the backend protocol; class declarations do not yet
support protocol bounds themselves.

`send` checks the open state and UTF-8 byte limit before invoking the backend.
Only `Ok` increments the successful-message count. `Err` leaves the count
unchanged and closes the wrapper: the backend may already have emitted bytes,
and a message boundary is no longer assumed trustworthy. The toy error does
not report a resumable offset. A production transport must either specify an
all-or-nothing enqueue contract or preserve partial progress and recovery state,
as [std.io](../design/stdlib/io.md) does. Hidden hooks do not resolve this choice.

There is no inheritance or protocol default body here. `send_impl` is absent
from `Transport[B]`, and the application cannot reach `_backend`. A backend
method also named `send` cannot replace the wrapper's method. No `fixed` or
`final` annotation is needed. Public functions accepting a structural `Sendable`
instead of the nominal wrapper would allow different algorithms; choose that
contract only when such substitution is intended.

The remaining difference from private virtual hooks is precise: an application
can construct a *separate* `Memory` and call its public `send_impl`. It cannot
use that value to access the stored backend owner. Separate handles can still
alias the same socket, database or other external resource; field privacy is
not proof of exclusive access to that resource. The wrapper's contract must
account for permitted sharing. A library that only promises wrapper-local
invariants can stop here.

## Capability variant

The example also shows a public `Permit` type with a private constructor,
passed as `&Permit` to the public hook. The library creates it only after
validation. An external backend author can name `Permit` and implement the
protocol; the application author cannot manufacture it through either constructor
spelling. This separates the ability to supply the operation from possession
of the authority needed to invoke it. The hook remains publicly nameable and
callable by code that already has a permit; it is not a private method.

A borrowed argument avoids transferring ownership of the permit. Current
Plenty disallows stored reference fields, and the hook's result contains no
returned borrow. However, ordinary classes support explicit `copy`, including
their private fields. The tested variant adds a no-op private `__del__` so
`copy(permit)` is rejected under today's custom-destruction rule. That is
workaround boilerplate, not a proposed long-term spelling for noncopyable types.
The compiler's borrow rules also forbid escaping a captured borrowed permit;
there is no permission to store a reference merely because it is in a closure.

A permit gates possession during the operation. It does not bind authority to
one receiver or message, enforce exactly one nested call, or prevent deliberate
delegation to another backend while the borrow is live. The library must not
publish a permit factory/accessor, accept an application callback with a permit,
or return a permit in an error if it wants this boundary. Token-bearing hooks
also become part of the public implementor contract even when omitted from
application documentation.

For transaction-sensitive hooks, use a transaction-specific authority rather
than treating this generic send permit as proof of an active transaction.
Represent the connection/transaction identity and allowed operations explicitly;
a dummy token proves only possession.

## Storage and transaction ownership

For `Store.save(record)`, the library author owns validation and the transaction
algorithm, a SQLite backend author supplies storage-specific operations, and
the application author calls `save`. An in-memory backend follows the same
contract. A plausible algorithm is:

```text
validate record
acquire transaction (failure: no hook, no commit)
call storage hook with transaction context
on hook success: explicitly commit
on hook failure: explicitly roll back and retain both errors if rollback fails
on commit failure: follow the documented retry/rollback policy
```

This is an algorithm sketch, not new Plenty syntax or a working generic SQLite
adapter. Beginning, committing and rolling back are themselves backend-specific;
a single hidden `write_impl` does not make them backend-independent. A concrete
transaction owner or backend protocol must supply those steps too.

The real [SQLite transaction](../design/stdlib/sqlite.md) remains open after a
busy commit so it can be retried, and rolls back when dropped unfinished.
Choose explicitly whether a library wrapper retains that owner for retry or
rolls back before returning an error. Never report commit success merely because
the write hook succeeded. A borrowed transaction context can restrict writing to
the transaction lifetime, but must also carry meaningful backend authority.

[Resource scopes](../design/language/08-errors/03-resource-scopes.md) supply
cleanup on ordinary exits; `__exit__` returns unit and has no success/failure
argument. It cannot replace explicit fallible commit/rollback handling or infer
that a body succeeded. Traps do not unwind it. Coarse distinct transaction/state
owners may enforce sequencing better than method hiding; see
[typestate](typestate-and-owned-transitions.md). Ownership transfers, retryable
failure and extraction of affine fields still need an API design.

## What the stdlib actually needs

| Current source | Algorithm and extension boundary | Assessment |
| --- | --- | --- |
| `stdlib/std/io.plenty`: `Reader`, `Writer`, `read_exact`, `write_all` | Public one-attempt operations; free helpers add retry, bounds/progress validation and partial-failure policy | Partial operations are useful application APIs. Do not make every stream operation a hidden hook. |
| `stdlib/std/http.plenty`: `BodyReader.read_body`, private `step` | Wrapper checks ranges, decodes framing and poisons its state on failure; external streams supply `Reader.read` | Composition already protects this algorithm. `step` is a library-owned shared helper, not an external hook. |
| `stdlib/std/io.plenty`: `BufReader.inner` | Public underlying owner intentionally permits closing/replacement, with a warning about bypassing buffered bytes | Choosing public escape access is an API decision. An access feature cannot repair a deliberately exposed field. |
| `stdlib/std/db/sqlite.plenty`: `Transaction` | Concrete transaction owner with commit/rollback lifecycle | Strong motivation for explicit transaction authority and failure semantics, not evidence for private protocol hooks. |

These sources were inspected at `5346f85`. No stdlib conversion follows from
this investigation.

## Alternative: explicit protocol access grants

If later evidence requires *lexically hidden* external hooks, the narrowest
candidate would keep the ordinary wrapper and add an explicit mapping from a
backend's private method to a public protocol requirement. Only the backend
type's defining module could issue that grant, for a named protocol identity
and concrete arguments; the library could not grant itself access by declaring
a matching signature. This is a semantic candidate, not proposed syntax.

A coherent version must confine invocation to the protocol's owning module,
not every generic function declaring the bound. Otherwise an application could
write its own bounded trampoline and regain access. The access is to the listed
method only, never other private members or the whole backend module; aliases,
imports and specialization must not broaden it. Ordinary accessible methods
would keep structural matching. A private requirement would need the explicit
grant, and the same method call in an unauthorized module must fail even if
conformance is known. Hidden type names in public signatures remain prohibited;
do not silently admit private concrete backend types as a side effect.

This creates two conformance/access paths and provenance that specialization
currently erases. It also couples a backend to a library protocol identity and
requires coherent duplicate/missing/signature-mismatched grant diagnostics.
Inference must check authority before using a private signature as evidence;
it must never search private methods or guess an implementation. A useful error
must identify the requiring call, protocol, candidate method and absent grant,
without simply suggesting `pub` where hidden access was intended.

No inheritance, runtime protocol object, default method body or override system
is needed. Their addition would increase scope without fixing access. Generic
specialization can preserve today's static dispatch, borrowing and allocation
costs, but new access provenance would affect checking, caching and diagnostics.
That cost is not justified by the current examples.

## Tradeoffs and remaining evidence

The plain wrapper adds one owner and a small bounded delegation helper, keeps
normal structural inference and requires no runtime allocation. A permit adds
one public type and hook parameter, private construction and the current copy
barrier; it makes capability errors visible at constructor/arity checks. Both
retain ordinary exclusive backend borrows and Result-based cleanup.

Protocol grants could remove token plumbing and keep hook names off application
call surfaces, but introduce a new access relation and implementor obligations.
Neither approach proves semantic hook correctness. Choose a token when possession
or transaction context is useful in its own right; use plain composition when
the wrapper owns the only backend application code needs.

The decision against a new feature can be revisited with an external-backend API
whose real invariant fails under the baseline, or measured token/delegation
friction that exceeds the cost of grants. Identity-scoped transaction capabilities,
noncopyable declarations, diagnostic ergonomics and compile-time measurements
remain unanswered design questions, not scheduled implementation work.
