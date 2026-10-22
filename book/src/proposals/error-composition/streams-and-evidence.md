# Error-composition experiments and typed streams

**Status: Partly implemented.** This is the evidence companion to the
[composition overview](../error-composition.md), not a separate proposal or
work queue. Mapping and the additive typed body-transfer slice have shipped;
a complete typed HTTP client migration has not. Current contracts belong to
[`std.error`](../../design/stdlib/error.md),
[`std.io.typed`](../../design/stdlib/io-typed.md), and
[`std.http.typed`](../../design/stdlib/http-typed.md).

## Prototype findings

The first implementation needs no compiler or runtime changes. `map_error`
uses `OnceCallable`, a mutable local callback binding, and an ordinary match.
The mutable binding accepts stateful reusable callbacks as well as named
functions and consuming closures. A callback is owned even when unused, so
success drops its captures. Error values need no reporting protocol.

The initial representative stdlib changes covered three result adapters:
`std/http/body.plenty` removes `json` and maps directly in `read_json` and
`send_json`; `std/http/client.plenty` uses mapping in `framed`; and
`std/json.plenty` uses it in `faulted`. Three explicit success-forwarding
branches disappear. Two convenience adapters remain and one non-generic
`bytes_fault` classifier exposes the JSON policy. The number of named
functions is therefore unchanged, while one adapter disappears completely.
Other conversion helpers initially remained available for comparison.

A wider initial conversion of eleven adapters exceeded the compiler's global
256 generic-function specialization cap in the offline HTTP JSON service test.
Keeping a generic convenience adapter around `map_error` adds another instance
for each success/callback type. The follow-up for issue #141 separates a
256-level dependency-depth guard from a 16,384-instance total resource budget;
independent specializations no longer consume the depth guard. It restores the
wider eleven-adapter adoption: body allocation, byte classification, HTTP and
storage conversions; client allocation; and JSON allocation and location-free
errors now also use mapping. Their conversion policies are unchanged. This
addresses the observed scaling constraint without removing finite compiler
limits or introducing allocation. See [specialization limits](../../design/toolchain/02-architecture/emission.md).

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
failure-side attribution, and typed transport preservation. These findings
supported the additive production framing below; mapping remains independently
usable.

## Additive production slice

[`std.io.typed`](../../design/stdlib/io-typed.md) now makes the prototype's typed
protocols and progress helpers available without copying example modules.
Its owned legacy reader/writer bridges preserve the returned `StreamError`
and classify only explicit interruption as retryable. A consuming free function
recovers each bridge's stream for explicit flushing, closing or seeking.
The private optional slot supports extraction under existing field-move rules;
it allocates no storage.

[`std.http.typed`](../../design/stdlib/http-typed.md) initially provided fixed-length and
close-delimited body readers. Close-delimited EOF is sticky, empty reads do not
consume input, and its connection is never reusable. Both return
`FramingError[E]` through the typed protocol without error-storage fields or
retrieval methods. The production tests run the prototype's cause/progress
cases against both implementations and exercise real `std.io.Cursor`
bridges through both framing modes. The standalone `standard.plenty` example
is the public-module counterpart to `main.plenty`.

The subsequent chunked reader preserves typed transport errors and payload
progress through size lines, data CRLF and bounded trailers. It retains parsing
state across interruption and does not read into the next message. Framing and
allocation failures use `FramingError.Protocol(HttpError)`; the concrete
transport cause stays in `Transport(E)`. Its scripted regression tests inject
interruptions and transport errors throughout metadata and payload, and check
limits, malformed input and terminal-state behavior. The original isolated
prototype remains a fixed-body experiment rather than a duplicate chunk parser.

This is still a body-transfer slice: validated heads, TLS
setup, deadlines, connection recovery and client migration remain outside its
contract. Existing HTTP adapters retain all their side channels; bridging them
does not reconstruct causes they already discarded. No builtin Failure,
general-sum, or reporting change is needed by these modules.

On 2026-10-18, a local unoptimized compiler comparison of `main.plenty`
against `standard.plenty` used five `--compile` runs each with
`PLENTY_CACHE=off`. Median wall time was 0.196 s versus 0.216 s (ranges
0.163–0.209 s and 0.181–0.225 s); linked executable sizes were 3,286,648
and 3,290,816 bytes. Other validation builds were running concurrently, so
these timings are a bounded smoke measurement, not a performance guarantee.
The public fixed path adds roughly 4 KiB in this probe; it imports the legacy
stream module to define bridges. Both copy signatures still carry two error
types and two stream types, while fixed framing carries one error and one
reader type. Call sites infer these parameters. No specialization-count
instrumentation was added, and this small case does not establish scaling for
a complete HTTP client.
