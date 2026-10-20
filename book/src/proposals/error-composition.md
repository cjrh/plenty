# Explicit error composition

**Status: Partly implemented.** Library mapping, general anonymous sums,
human-readable reporting, and additive typed stream/body APIs have shipped.
`?` supports lossless error-set widening, without user-defined conversions;
builtin `Failure` erasure stays as a last resort, for when no precise error
can be written (issue #342).
Remaining work belongs in [B71](../backlog.md).
The companion [error sets](error-sets.md) page records the sum contract and
the last-resort role of builtin `Failure`; [formatting](completed/error-formatting.md)
records the shipped enum-capable reporting design.
[Typestate](typestate-and-owned-transitions.md) addresses lifecycle invariants.

## Reading this family

| Question | Design and current contract |
| --- | --- |
| How do I deliberately translate an error? | Mapping below; [`std.error`](../design/stdlib/error.md) |
| How do I retain several possible error types? | [Anonymous-sum design](error-sets.md); [current sums](../design/language/03-types/anonymous-sums.md) |
| How do I present an error to a person? | [Completed reporting design](completed/error-formatting.md); [`std.report`](../design/stdlib/report.md) |
| How do stream adapters preserve a transport cause? | [Experiment and production evidence](error-composition/streams-and-evidence.md); [`std.io.typed`](../design/stdlib/io-typed.md), [`std.http.typed`](../design/stdlib/http-typed.md) |

## Problem and recommendation

At the original investigation baseline, `?` propagated an identical error type,
except for deliberate erasure to `Failure`. Library adapters repeated success forwarding just to
translate failures. A second, independent problem is that `std.io.Reader`
fixes its error type to `StreamError`: HTTP adapters store richer causes in a
separate field and return a generic error through the stream interface.

Explicit result mapping and an isolated parameterized-stream experiment now
provide the baseline. General anonymous sums and human-readable formatting
for enums and classes have since shipped. Builtin `Failure` stays as a last
resort; issue #342 removes the export and retained-job cases that forced it.
Keep named domain errors where they encode useful
policy. Production stream migration remains independently reviewable; neither
mapping nor sums alone can recover causes discarded by existing stream APIs.

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
| `E: Error` generic constraint | Operations available on one statically known `E` | Classes and enums can satisfy protocols; a reporting bound is optional |
| `Result[T, Error]` holding arbitrary implementations | An owned existential value with hidden concrete type | Not supported; protocols are not runtime value types |
| `Result[T, A | B]` | One of a finite, statically declared set | Implemented anonymous sums; see [the companion page](error-sets.md) |

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
The implemented [formatting protocol](completed/error-formatting.md) provides
presentation without an existential error container. Enums can satisfy protocols,
and anonymous sums forward this formatting protocol when all members qualify.
Builtin Failure stays as a last resort, for when no precise error can be
written (issue #342); an erased error container and heterogeneous cause-chain interface remain deferred.

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

The [experiment record](error-composition/streams-and-evidence.md#prototype-findings)
records mapper adoption, specialization limits, and the typed-stream prototype.
It distinguishes the isolated examples from the public library APIs.

## Additive production slice

The [production slice](error-composition/streams-and-evidence.md#additive-production-slice)
provides typed I/O helpers and fixed-length/close-delimited HTTP body readers.
Full client migration, chunked framing, and removal of legacy side channels
remain separate work; the evidence page records the measured scope and limits.
