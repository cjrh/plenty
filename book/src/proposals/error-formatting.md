# Human-readable formatting and error reports

Status: not implemented. An enum-capable formatting protocol and its interaction
with [anonymous sums](error-sets.md) are the agreed direction. Exact public API
names and formatter operations remain to be specified before implementation.
Current behavior is defined by [entry reports](../design/toolchain/01-building/entry-report.md),
[formatting](../design/facilities/01-builtins/formatting.md), and
[`std.error`](../design/stdlib/error.md). Scheduling belongs in B71 in the
[backlog](../backlog.md).

## Separate data, presentation, and reporting

An error stays ordinary typed data. Neither `Result[T, E]` nor `?` requires a
reporting protocol. Finite sums compose possible causes without deciding their
human-readable wording or recovery policy.

Structural representation remains available for debugging and fallback reports.
A general human-readable formatting protocol lets classes and enums supply
messages. It is useful beyond errors; a nominal error marker is not required.
`Context[E]` and domain wrappers retain operation, path, and cause information.
Application reporting chooses destination, prefix, fallback, and exit status.
Formatting a value does not itself print or terminate the process.

Today entrypoint reports use structural rendering, with special explanations for
some builtins. `Context.describe()` also uses structural cause text, while
`describe_with()` accepts caller-provided wording. There is no customizable common
formatting protocol. Existing protocol bounds accept classes; supporting enums
is an explicit prerequisite, not something these APIs already provide.

## Borrowed formatting into a destination

The protocol borrows its receiver and writes text through a mutable formatter.
An enum implementation matches its variants and formats their details. It does
not consume the error or require constructing an owned string. A convenience API
may collect the same output into a fallible allocated string.

The core formatter must support an allocation-free reporting path for fixed
messages and scalar payloads. This is not a promise that every user-defined
implementation allocates nothing: custom computation and string collection keep
their ordinary fallibility. Formatting errors must be explicit, must not replace
the original application error, and must terminate reporting rather than recurse
into reporting the formatting error itself.

The implementation contract must define text/scalar writes, destination failure,
the result type of the formatting method, and which operations may allocate.
Prefer a small formatter abstraction over making every error generic over all
I/O writers. Its storage and dispatch must be specified without smuggling in a
boxed runtime protocol value. This page does not choose a public method spelling
before those signatures and borrowing rules are reviewed together.

## Enum protocols and sum formatting

Extend ordinary protocol satisfaction to enums, with enum methods subject to
the same visibility, receiver, ownership, and effect rules as class methods.
No special error-only implementation mechanism is needed.

For the formatting protocol, `A | B` satisfies the protocol exactly when every
normalized member satisfies it. Generated dispatch examines the tag and borrows
the active payload; it adds no allocation or hidden existential container.
Aliases and singleton normalization must give the same formatting behavior.

This rule does not automatically forward arbitrary protocols through sums.
Methods involving `Self`, mutation, or different result types require a separate
general contract. The first version grants only the specified formatting behavior.

## Reporting at the application boundary

The target entrypoint policy chooses human-readable formatting when the concrete
error type satisfies the protocol, otherwise structural representation. For a sum
with an unformattable member, use structural reporting for the sum as a whole;
do not introduce a hidden per-member relaxation of the protocol bound.

If the selected rendering fails or is unavailable, emit a fixed minimal report
on a best-effort basis. The original error is destroyed once and the failing
entrypoint still returns status one. Intermediate propagation remains silent.
Output failures, including a closed stderr pipe, must not trigger another report.
Preserve the existing entrypoint treatment of stderr failure.

Streaming can fail after emitting a prefix or part of a message. The detailed
reporter contract must define a bounded fallback suffix/newline for that case;
it cannot promise atomic output without providing buffering. Do not retry a
user formatter after partial output. Ordinary traps retain their existing
non-unwinding semantics; a best-effort reporter cannot catch arbitrary traps.

Adapt context formatting to present the operation/path and then the typed cause.
Keep explicit caller-provided descriptions useful. Avoid duplicating the cause
in both a wrapper's message and an automatically appended chain. A heterogeneous
`source()` interface, dynamic error objects, and general cause-chain traversal
are deferred; typed context composition does not depend on them.

## Acceptance criteria and decisions before coding

Check class and enum protocol satisfaction, missing-method diagnostics, borrowed
payload preservation, sum dispatch for every member, singleton/alias behavior,
and rejection of a formatting bound when any member lacks it. Verify that plain
unformattable data remains legal in `Err`.

Exercise fixed messages under forced allocation failure, scalar formatting,
string collection failure, nested contexts, custom formatter failure before and
after output, unsupported structural rendering, closed stderr, exactly-once
payload destruction, and identical compile-and-run/native executable reports.

Resolve the formatter method/result/destination API, partial-output fallback
wording, enum method declaration syntax, and context convenience API compatibility
before coding those interfaces. These are remaining design details within the
agreed architecture. The implementation must update the reference and tutorial
with actual syntax and behavior; this proposal does not change current APIs.
