# Bundled HTTP acceptance

> **Status: Not implemented.** No HTTP/network/TLS module ships today. Existing
> [concurrency](../design/facilities/03-concurrency/index.md) and
> [I/O](../design/facilities/02-io/index.md) references own the primitives and
> their current limits. Issue #49's production-server requirement is preserved
> here; delivery and dependencies belong only to [B58–B66 in the backlog](../backlog.md).
> HTTP names/signatures and native backend selection remain design candidates.

The bundled server should remain usable in production without requiring a
third-party replacement for a demonstration server. Go's combined HTTP
client/server surface motivated this goal in issue #49; no Go compatibility
or claim about its protective defaults is implied.

## Execution and ownership

Define trusted native I/O effects and transferable/thread-safe handle ownership
before scheduling socket/TLS calls in workers. Current worker restrictions do
not permit arbitrary native calls or files. Preserve ordinary checks for std;
never exempt the whole namespace. A measured bounded blocking model may be
sufficient initially; polling is an alternative for many idle connections.
Unbounded threads per connection are not an acceptable default. Async language
syntax is not a prerequisite.

Handlers and shared state must obey ownership/thread-safety rules. Basic routing
must work without heterogeneous erased closures. Shutdown deadlines must not
reclaim borrowed storage still in use. Fatal traps currently do not unwind:
expected handler failures use Result, while arbitrary fatal traps remain
process-fatal. Per-request recovery cannot be promised by a catch-all mechanism.

## Client and wire contracts

Use HTTP/1.1 as the initial conformance boundary. Bound header count/bytes,
body/buffer sizes and parser work. Reject malformed/conflicting length or
transfer encoding, truncated bodies and invalid headers with protocol errors,
not unbounded allocation or accidental traps. Define reuse/framing policy and
streamed request/response bodies with partial-progress I/O.

The reusable client needs pooling, DNS/connect/TLS/request deadlines,
cancellation, redirect/proxy policy and bounded streaming. Certificate/hostname
verification is enabled by default. Avoid automatic retries once request
effects may have happened. Maintain backend/trust-root versions and updates;
tests use local certificates/services.

## Server and deployment contracts

Provide method/path routing, composable handlers, JSON/form helpers, cookies
and bounded multipart parsing/storage. Bound connections and queued work with
backpressure and documented overload behavior. Specify header/idle/handshake
and body/write deadlines plus explicit controls for long-lived streams.
Cancellation/failure releases handles and multipart temporary storage.

Support direct TLS and documented reverse-proxy deployment. Forwarded identity
or addresses require an explicit trusted-proxy policy. If static-file helpers
are adopted, enforce filesystem-root/symlink rules with filesystem-aware checks;
string cleanup alone is insufficient.

Graceful shutdown stops accepting, drains or cooperatively cancels active work
under a documented deadline policy, and releases resources after safe joining.
Provide structured access/error events and metrics hooks with caller-selected
sinks and no mandatory telemetry vendor.

## Qualification gates

Production qualification is a separate issue from a core server implementation.
Require parser fuzzing and retained regression corpora, protocol conformance/
interoperability, slow-client/cancellation/allocation-failure tests, sustained
load and resource-leak tests, and repeatable throughput/latency/memory baselines.
Publish workloads, hardware, versions, supported platforms and configured
limits. Exercise overload and graceful shutdown while work is active.

Run the motivating CLI and JSON service integration examples with an installed,
relocated toolchain offline, including verified TLS and proxy fixtures. Check
unused-module/backend linking. Document expected failures, fatal boundaries,
operational guidance and an ongoing security-fix/maintenance process before
calling the server production-ready. HTTP/2 and HTTP/3 remain separate decisions.
