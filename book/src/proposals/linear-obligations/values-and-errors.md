# Linear values, aggregates, and errors

Status: **Not implemented; companion to [linear obligations](../linear-obligations.md).**
Issue #149, [B72](../../backlog.md). This page is part of the core ownership
proposal, not an independent feature. The current
[sums](../../design/language/08-errors/01-sums-and-propagation.md) and
[ownership](../../design/language/05-ownership/01-bindings-moves-and-copies.md)
contracts remain authoritative. All examples are speculative pseudocode.

## Wrapping preserves responsibility

Discardability is structural and conservative: a tuple, enum, class, box,
collection, closure, or error that can own a linear value cannot be freely
discarded. A nominally linear wrapper additionally retains its own seal.
Wrapping, allocating, or erasing representation must not make an obligation
disappear. Recursive types need a fixed-point containment calculation.

| Owner | Permitted handling / required restriction |
| --- | --- |
| Tuple | Move as a whole or unpack every component; ignored linear components are errors. |
| Enum / `Option` / `Result` | Match by consumption and handle each payload. A payload-free arm can finish normally after the match proves that arm. |
| Class / box | Transfer whole owner, or use checked consuming deconstruction. A custom destructor is not permission to drop linear fields. |
| List / dictionary / set | Insert and remove by transfer with ownership-preserving failure. Clear, eviction, replacement, duplicate-key/value rejection, and container drop require discardable discarded items or an explicit resolver. |
| Capturing closure | Capturing by move transfers the obligation. A one-shot call must resolve/return it; destroying an uncalled closure is rejected. Borrowed captures keep existing lifetime rules. |
| Reusable closure | Invoking it does not consume its environment. A linear capture remains owed even after calls; an explicit final consuming operation would be needed. |
| Lazy generator / erased callable | Reject linear captures, retained locals, or erased obligations until their destruction and suspension contracts preserve them. The current generator drop does not run the rest of its body. |

`Option[Transaction]` is conservatively non-discardable even if a particular
value is `Nothing`. Consuming matching proves the payload-free branch locally;
it does not grant the entire type discardability. Similarly an empty linear
collection needs a checked consuming empty-finalization operation, rather than
a rule that drops arbitrary `list[Transaction]`. Returning an iterator's
unconsumed remainder preserves ownership; early break from an iterator that
silently destroys the remainder must be rejected. Borrowed iteration is useful
without consuming anything, and consequently does not finish the collection.

Existing generic bodies that may discard a `T` need a `Discardable[T]`
requirement. Transfer-only identity, pairing, and error forwarding need none.
Instantiations with ordinary existing types retain today's behavior. Initially
the existing specialization machinery can check the requirement at use and
point to the discarding body; explicit bound syntax and declaration-time generic
checking are separate ergonomic decisions. Containers themselves need not
require discardable elements: only operations which destroy elements do.

This affects builtin APIs, not just user-defined generic functions:

- `unwrap_or(default)` discards the default when a present payload wins;
  the existing signature needs discardable `T`. Use an explicit match that
  handles both owners, or a new API returning the unselected alternative.
- `ok_or(error)` drops the error on `Some`, so requires discardable `E`; it
  may still transfer a linear `T` into `Ok`. Construction of a temporary option
  or error follows the same failure rules.
- `is_ok()` and similar observers do not discharge obligations. Observing a
  linear temporary and then destroying it is an error.
- `map_error` must check the mapper's handling of the owned error. Mapping a
  live-transaction error to an ordinary code cannot silently erase its payload.

## Allocation and partial construction

A consuming allocating API cannot simply return `AllocError` after losing its
input. It must reserve capacity before transfer, preserve caller ownership via
an explicitly checked operation, return the owner in the failure payload, or
invoke an authorized resolver under a documented policy. An ordinary owned call
transfers its arguments: restoring a moved caller binding is not implicit.

For example, speculative `insert_owned(list, item)` may return either the
updated list or `(list, item, AllocError)`. An insertion through a borrowed list
can return the uninserted item on failure. Dictionary insertion must return or
resolve a replaced value and any unused incoming key. Constructors must account
for earlier initialized fields when a later field expression propagates error.
Closure allocation failures must return owned captures or occur before transfer.
Hidden builders and argument temporaries are subject to the same rules.

For an existing native operation whose failure path releases an argument,
reject linear instantiation until its contract is extended. “The allocator
failed” is an ordinary error, not permission to abandon protocol state.

## Transaction ownership on failure

Consider the intentionally rejected pseudocode:

```text
tx = begin_transaction(connection)?
insert(&mut tx, record)?       # error returns while tx is still owed
commit(tx)?
```

Handle insertion failure by explicitly rolling back, returning an error that
owns `tx`, or entering a checked [resolution scope](../linear-scope-resolution.md)
before the fallible operation. Acquisition must also specify ownership on
failure: a consumed connection is returned or terminally closed; a borrow stays
with its owner. No hidden success-only ownership contract is sufficient.

Commit APIs have two different valid shapes:

```text
commit_local(tx: Transaction) -> Result[Receipt, TerminalCommitError]
# Every result has resolved the local obligation; no retry owner survives.

commit_retryable(tx: Transaction) -> Result[Receipt, CommitError]
CommitError = Retry(Transaction, DbError) | Finished(TerminalCommitError)
# Retry retains the live transaction; Finished does not.
```

These are alternative contracts, not two names for the current SQLite method.
The [existing typestate assessment](../typestate-and-owned-transitions.md)
explains why SQLite's busy commit must remain retryable. Its current mutable
method already preserves the transaction; a consuming redesign must carry it
in the retry result. The error enum above is itself non-discardable.

```text
match commit_retryable(tx):
    Ok(receipt): return Ok(receipt)
    Err(Retry(tx, cause)):
        return Err(NeedsDecision(tx, cause))   # caller now owns the obligation
    Err(Finished(cause)):
        return Err(TerminalFailure(cause))
```

A matching Result error type may propagate this obligation with `?`, provided
other live locals are handled. Propagation to current `Failure.Unspecified`
must be rejected when it would destroy an obligation. A diagnostic should name
the retained transaction, the error conversion, and the original error type;
suggest preserving the typed error or resolving its owner first. Matching a
terminal arm can still permit ordinary error erasure afterward.

Rollback is not automatically infallible. A retryable rollback must likewise
return ownership; an authorized local-release operation can instead finish
with a reported failure. “Local handle released” and “remote rollback confirmed”
are different contracts. For a remote transaction with lost acknowledgements,
an `UnknownOutcome` may be a terminal *local* outcome if the API explicitly
permits it. If reconciliation is required, it must carry a live recovery
obligation. The type system cannot invent knowledge the protocol lacks.

## Conformance probes

Accept typed retry errors passed through several helpers, complete enum matches,
owned removal with remainder returned, and allocation failure returning every
input. Reject nested wrappers dropped through a generic helper, a discarded
closure containing a transaction, wildcard patterns hiding obligations,
replacement of a linear dictionary value, and partial iteration losing its
remainder. Test both `unwrap_or` branches and `ok_or`'s unused error path.

Failure injection must cover every allocation point after the first transfer,
including partially built objects and closures. Diagnostics should trace the
containment path (for example `SubmitError.job.capture.tx`) and the cleanup
edge, rather than merely saying a large aggregate is noncopyable. Discardability
is the missing capability; copying and borrowing are separate concerns.
