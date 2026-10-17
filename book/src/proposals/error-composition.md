# Explicit error composition

Status: library mapping implemented; typed streams prototyped outside the
stdlib API. The `?` error contract is unchanged. Remaining work belongs in [B71](../backlog.md).
The companion [error sets](error-sets.md) page evaluates anonymous sums;
[typestate](typestate-and-owned-transitions.md) addresses lifecycle invariants.

## Problem and recommendation

Plenty's `?` propagates an identical error type, except for deliberate erasure
to `Failure`. Library adapters consequently repeat success forwarding just to
translate failures. A second, independent problem is that `std.io.Reader`
fixes its error type to `StreamError`: HTTP adapters store richer causes in a
separate field and return a generic error through the stream interface.

Recommend explicit result mapping first, and an experiment with parameterized
stream errors second. Neither requires dynamic error objects or anonymous sums.
Keep named domain errors where they encode useful policy. Evaluate error sets
separately rather than making them a prerequisite for these improvements.

## Evidence in the stdlib

Locations below describe the inspected baseline, commit `bef41b1`; function
names are the durable anchors when line numbers move.

| Location | Existing burden | Candidate improvement |
| --- | --- | --- |
| `stdlib/std/http/body.plenty:79–140`, `allocated`, `http_allocated`, `lifted`, `http`, `json`, `stored` | Repeated `Ok` forwarding around error translation | Result mapping; retain classifiers for deliberate allocation normalization |
| `stdlib/std/http/client.plenty:333–387`, `alloc`, `bytes_of`, `framed`, `netted`, `parsed` | Several near-identical wrappers | Map only the failure payload |
| `stdlib/std/json.plenty:121–147`, `faulted`, `reserved`, `unplaced` | Nested adaptation before parsing can proceed | Shared mapping operation with named classifiers |
| `stdlib/std/http/client.plenty:599–640`, `Wire.noted`, `fault`, `written` | Error stored in `_failure`, generic stream error returned, cause retrieved later | Preserve the transport error as part of the returned type |
| `stdlib/std/http/client.plenty:690–707`, `Response.read`, `take_failure` | Caller must retrieve richer error separately | Typed stream error path with optional compatibility adapter |
| `stdlib/std/http.plenty:633–647`, `BodyReader.read` | Framing faults become `Io(InvalidInput)` plus side storage | Parameterized framing/stream error |
| `stdlib/std/error.plenty:62–85`, `context`, `path_context` | Already preserves a typed cause and handles failed context allocation | Reuse this foundation; do not replace it with unconditional boxing |

These are opportunities, not claims that all conversions can disappear.
`BytesError.InvalidUtf8` becoming `BodyError.InvalidUtf8`, for example, is
intentional public API policy.

## Explicit mapping

Possible future method spelling (not current executable Plenty):

```text
body = std.json.encode(value, limits).map_err(body_json_error)?

def body_json_error(cause: JsonError) -> BodyError:
    BodyError.Json(cause)
```

The operation consumes `Result[T, E]` and a mapper constrained by
`F: OnceCallable[[E], G]`, returning `Result[T, G]`.
The implemented `std.error.map_error(result, mapper)` provides these semantics
as an ordinary library function. Method syntax remains an ergonomic choice,
not required semantics. See [the reference](../design/stdlib/error.md#explicit-error-mapping)
and [the tutorial](../tutorial/03-absence/error-details.md).

- Evaluate the result and mapper expressions once, left to right.
- Invoke the mapper exactly once on `Err`, never on `Ok`.
- Move the original error into the mapper; forward success without copying.
- Drop an unused mapper and its captures on success. Ownership and early-exit
  cleanup apply normally, including failure while evaluating the mapper.
- Mapping itself allocates nothing. Callback construction and execution retain
  their ordinary effects; do not silently box captured callbacks.
- A mapper returns `G`, not a secretly flattened `Result[G, X]`. Fallible
  context enrichment needs an explicit API and policy for preserving the cause.
- `?` still performs no user-defined conversion. The call site names the policy.

This removes repetitive success branches while leaving semantic conversions
reviewable. A helper can still be worthwhile when it communicates a domain
decision. Do not mechanically inline all classifiers or introduce complex
closures solely to reduce line count.

Rust's `thiserror` generates error implementations and selected conversions;
it reduces declaration boilerplate while retaining a named error type.
Plenty could later consider declared conversions, but implicit conversion at
every `?` would need ambiguity and coherence rules. Explicit mapping is the
smaller initial contract. [thiserror documentation](https://docs.rs/thiserror/latest/thiserror/)

## Does an error protocol solve it?

The idea is coherent, but has three distinct meanings:

| Meaning | What it provides | Fit today |
| --- | --- | --- |
| `E: Error` generic constraint | Operations available on one statically known `E` | Protocol bounds exist, but currently accept classes; most stdlib errors are enums |
| `Result[T, Error]` holding arbitrary implementations | An owned existential value with hidden concrete type | Not supported; protocols are not runtime value types |
| `Result[T, A | B]` | One of a finite, statically declared set | New sum-type composition feature; see companion page |

A generic bound does not allow one specialization to return unrelated concrete
types on different branches. It still needs an enum, union, or erased container.
Nor does a common reporting interface establish how to classify interruption,
retryability, partial progress, or a framing failure.

An existential error container must specify storage, destruction, dispatch,
thread transfer, reporting, and whether downcasting is supported. Arbitrarily
large owned payloads cannot all fit a fixed-size inline object; boxing or an
explicit capacity/fallback policy is needed. Allocation while reporting an
allocation failure is a particular problem for Plenty. Borrowed errors cannot
generally escape the failing function's locals.

Rust trait objects make runtime dispatch explicit. `anyhow` offers an erased
error with contextual reporting and downcasting, trading away an exhaustive
signature of possible causes. That can suit application boundaries, but it is
not equivalent to a finite typed sum. [Rust trait objects](https://doc.rust-lang.org/book/ch18-02-trait-objects.html),
[anyhow documentation](https://docs.rs/anyhow/latest/anyhow/)

Do not make a reporting protocol mandatory for error payloads: Plenty currently
allows ordinary typed data in `Err`. Reporting and recoverability are separate.
Keep `Failure`'s existing deliberate, allocation-free loss of detail distinct
from any future detail-preserving erased error.

## Removing the stream error side channel

Current [parameterized protocols](../design/language/07-generics/protocols.md)
already substitute type arguments in result positions. A minimal `Source[E]`
and generic reader using an enum error type also passed the local debug compiler
smoke check. Prototype the following
shape in an isolated library module, before changing `std.io`:

```text
protocol Reader[E]:
    def read(self: &mut Reader[E], buffer: &mut Bytes,
             start: i64, stop: i64) -> Result[i64, E]:
        pass

enum ReadFailure[E]:
    Source(E)
    UnexpectedEof
    InvalidProgress
    OutOfRange
    Allocation(AllocError)
```

This is a proposed interface shape, not a claim that changing `Reader` alone
is sufficient. Helpers generate their own errors, and retry logic needs a way
to recognize interruption. Keep that policy explicit: use a classifier callback
or a protocol parameter for a classifier class, or require a normalized stream
envelope that distinguishes `Interrupted` from `Source(E)`. Compare these
options against concrete adapters before selecting one.

`read_exact[E, R: Reader[E]]` would return progress plus `ReadFailure[E]`;
`copy_all` must preserve independent reader/writer error types and the failing
side. HTTP framing would similarly need `HttpError[E]` (or a distinct generic
framing error), separating transport causes from protocol failures. Existing
`StreamError` is still useful for OS adapters and compatibility facades.

Avoid recursive type equations such as making `ClientError` contain
`HttpError[ClientError]`. Define a transport-only error underneath framing and
an exchange-level error above it. Separate EOF, malformed progress, cancellation,
and protocol faults; arbitrary `E` cannot be interrogated as if it were
`StreamError`. Parameter threading increases signatures and specialization;
measure whether eliminating side storage pays for that complexity.

Retain old APIs during a migration, or make a deliberate coordinated breaking
change. Remove `_failure`/`take_failure` only when every relevant adapter and
caller receives the full cause directly. Mapping alone cannot achieve this.

## Acceptance criteria

Compare before/after HTTP body adapters, a JSON classifier, and one complete
HTTP-over-stream path. Count adapter functions and side-channel fields, not just
lines. Verify exact cause preservation, once-only mapper invocation, success
without mapping allocation, destruction of affine payloads/captures, partial
progress, interruption, cancellation, and allocation failure. Check diagnostic
quality for incompatible error types and protocol inference. Tutorial/reference
changes belong with an implementation, not this proposal.

## Prototype findings

The first implementation needs no compiler or runtime changes. `map_error`
uses `OnceCallable`, a mutable local callback binding, and an ordinary match.
The mutable binding accepts stateful reusable callbacks as well as named
functions and consuming closures. A callback is owned even when unused, so
success drops its captures. Error values need no reporting protocol.

The representative stdlib changes cover three result adapters:
`std/http/body.plenty` removes `json` and maps directly in `read_json` and
`send_json`; `std/http/client.plenty` uses mapping in `framed`; and
`std/json.plenty` uses it in `faulted`. Three explicit success-forwarding
branches disappear. Two convenience adapters remain and one non-generic
`bytes_fault` classifier exposes the JSON policy. The number of named
functions is therefore unchanged, while one adapter disappears completely.
The other conversion helpers remain available for comparison.

A wider initial conversion of eleven adapters exceeded the compiler's global
256 generic-function specialization cap in the offline HTTP JSON service test.
Keeping a generic convenience adapter around `map_error` adds another instance
for each success/callback type. The final adoption is deliberately focused and
keeps the existing guard unchanged; broader library use needs a separate look
at specialization budgets and unnecessary wrapper layers. This is a concrete
scaling constraint, not evidence of recursive error types or hidden allocation.

Worker integration exposed a real ergonomic restriction: thin function values
lose their target identity inside the generic helper, so thread effect checking
rejects their indirect calls. The stdlib adopters use concrete consuming
closures (`def once (...)`) instead; their bodies remain inspectable and allocate
nothing. Named classifiers are called directly from these closures. This keeps
worker checks intact but adds a wrapper at those sites. Mapping does not solve
that existing callable-design limitation.

The isolated modules in `examples/error_composition/` explore a normalized
`StepError[E]` envelope: `Interrupted` requests retry; `Source(E)` preserves
everything else, including cancellation and allocation failure. `Reader[E]`
and `Writer[E]` feed `read_exact`, `write_all`, and `copy_all`; helper-generated
failures have their own variants. Copy errors distinguish reader and writer
types and count bytes delivered to the writer, including partial writes.
Caller-provided scratch storage avoids allocation in the helpers.

`FixedBody[E, R]` demonstrates Content-Length body framing over a typed
transport. Its failures distinguish `Transport(E)`, truncation, invalid
progress, invalid range, and reuse after failure. Interruption leaves it
usable; terminal framing/transport failures mark it broken. The full body-copy
path returns the original transport cause through `FramingError[E]` and
`CopyFailure[FramingError[E], W]`. It contains zero error-storage fields and
zero retrieval methods. Production `_failure` fields and `take_failure`
methods are unchanged: the experiment does not yet justify their removal.

This is one complete **body-transfer** path, not a complete HTTP client. It
assumes a previously validated Content-Length and response semantics. It does
not parse heads, implement chunked/close-delimited framing, perform TLS or
network I/O, or manage connection reuse. Scripted sources exercise rich TLS-like
errors and cancellation as typed data, not real handshakes or cancellation
tokens. Those boundaries must be covered before a production migration.

Protocol-bound inference recovers the error types from concrete methods, even
through the framing adapter: `fixed_body(source, length)` and
`copy_all(&mut body, &mut writer, &mut scratch)` need no explicit type arguments.
The generic definitions and nested error signatures are still long. A normalized
envelope keeps retry policy simple but
requires adapters to wrap their failures. A classifier callback would leave
transport signatures alone, at the cost of threading retry policy through
helpers; it remains an alternative, not implemented here. Arbitrary errors
must not be retried by guessing their meaning.

Focused tests live in `tests/test_error_composition.rs` and
`tests/test_typed_stream_prototype.rs`. They cover mapper ownership, allocation
failure, nested results, evaluation order, progress, retry, EOF, cancellation,
failure-side attribution, and typed transport preservation. The next decision
is whether the stream signature cost warrants a broader framing experiment;
mapping is independently usable today.
