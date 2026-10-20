# Human-readable formatting and error reports

> **Status: Implemented.** Enum methods, ordinary enum protocol satisfaction,
> the concrete [`std.report`](../../design/stdlib/report.md) formatting core,
> dispatch over anonymous sums, entrypoint selection, and typed context
> formatting have shipped. [Entry reports](../../design/toolchain/01-building/entry-report.md),
> [formatting](../../design/facilities/01-builtins/formatting.md), and
> [`std.error`](../../design/stdlib/error.md) define current behavior.
>
> Floating-point report formatting and dynamic cause-chain interfaces are
> extensions, not prerequisites for this completed core. Builtin `Failure`
> removal belongs to the separate [anonymous-sum migration](../error-sets.md).
> The [composition overview](../error-composition.md) explains how mapping,
> sums, reporting, and typed streams fit together; remaining work stays in
> B71 of the [backlog](../../backlog.md).

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

Entrypoint reports select `Formattable` when the concrete type satisfies it,
otherwise use structural rendering with special explanations for some builtins.
`Context.describe()` still uses structural cause text, while
`describe_with()` accepts caller-provided wording. `std.report.Formattable`
provides borrowed formatting into a concrete destination; entrypoint reports and
eligible `Context` values use it. Classes and enums can also supply
the existing `std.fmt` display/debug methods. Those methods use a generic writer
and allocated scratch and remain useful for richer application formatting.

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

The implemented contract is `Formattable.format(self, output: &mut Formatter)
-> Result[(), FormatError]` in `std.report`. `Formatter` stores a private inline
destination enum: stderr or an owned byte collector, plus byte/depth counters.
The stderr variant creates no scratch buffer or heap owner. Text, signed/unsigned
integers and booleans need no allocation; collection and `finish` explicitly
return storage failures. Floating-point spelling and rich format specifications
remain with `std.fmt` in this first core slice.

`render` borrows the value, checks its protocol and restores the depth counter on
success or error. Each native stderr attempt reports its exact byte progress;
interruption is retried, and other output errors make that formatter terminal.
No boxed protocol value, generic writer parameter, prefix, newline, exit policy,
or automatic fallback is hidden in this core. See its reference for byte limits,
partial output, and collection guarantees.

## Enum protocols and sum formatting

Ordinary protocol satisfaction now includes enums, with enum methods subject to
the same visibility, receiver, ownership, and effect rules as class methods.
Methods use ordinary indented `def` declarations alongside variants; shared and
mutable receivers borrow the enum. No special error-only implementation
mechanism is needed.

For the formatting protocol, `A | B` satisfies the protocol exactly when every
normalized member satisfies it. Generated dispatch examines the tag and borrows
the active payload; it adds no allocation or hidden existential container.
Aliases and singleton normalization give the same formatting behavior.

This rule does not automatically forward arbitrary protocols through sums.
Methods involving `Self`, mutation, or different result types require a separate
general contract. The first version grants only the specified formatting behavior.

## Reporting at the application boundary

The entrypoint policy chooses human-readable formatting when the concrete
error type satisfies the protocol, otherwise structural representation. For a sum
with an unformattable member, use structural reporting for the sum as a whole;
do not introduce a hidden per-member relaxation of the protocol bound.

If the selected rendering fails or is unavailable, emit a fixed minimal report
on a best-effort basis. The original error is destroyed once and the failing
entrypoint still returns status one. Intermediate propagation remains silent.
Output failures, including a closed stderr pipe, must not trigger another report.
Preserve the existing entrypoint treatment of stderr failure.

Streaming can fail after emitting a prefix or part of a message. Normal output
has a 16,384-byte budget and depth limit 64. A formatting failure appends the
fixed suffix ` [report unavailable]\n` outside that budget; a terminal transport
failure stops output instead. Reports are not atomic, and the reporter never
retries a user formatter after partial output. Ordinary traps retain their existing
non-unwinding semantics; a best-effort reporter cannot catch arbitrary traps.

Context formatting presents the operation/path and then the typed cause.
Explicit caller-provided descriptions remain useful. This avoids duplicating the cause
in both a wrapper's message and an automatically appended chain. A heterogeneous
`source()` interface, dynamic error objects, and general cause-chain traversal
are deferred; typed context composition does not depend on them.

## Acceptance criteria

Check class and enum protocol satisfaction, missing-method diagnostics, borrowed
payload preservation, sum dispatch for every member, singleton/alias behavior,
and rejection of a formatting bound when any member lacks it. Verify that plain
unformattable data remains legal in `Err`.

Exercise fixed messages under forced allocation failure, scalar formatting,
string collection failure, nested contexts, custom formatter failure before and
after output, unsupported structural rendering, closed stderr, exactly-once
payload destruction, and identical compile-and-run/native executable reports.

The existing context description/report convenience APIs preserve their behavior.
Conditional `Context[E]` formatting is a narrow compiler-supported rule for the
trusted standard type, rather than a general conditional-method feature. The
formatter method, result and destination API are defined in the `std.report`
reference; the entrypoint reference defines reporting limits and fallback.
