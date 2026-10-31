# Actor HTTP qualification

This record covers [`std.actor.http.server`](actor-http-server.md).
Its implementation and qualification are in progress. **It is not yet labelled
production-ready.** The [worker server's qualification](http-qualification.md)
is separate evidence and remains in force for that implementation.

## Gates

| Gate | Current evidence |
|---|---|
| Shared protocol grammar | Resumable decoder reuses existing head/target/framing rules; focused decoder and existing HTTP regression suites passed during development |
| Admission isolation | Actor regression tests cover constructor error/trap/empty result, restart-constructor failure, full parent mailbox, and heap/timer/descriptor recovery |
| Bounded teardown | Actor tests cover full-mailbox shutdown, idempotence, reuse, and repeated live-stage drop with trapping node destructors |
| Real HTTP/TLS connections | `test_std_actor_http_server` passed with runtime/allocation checks, including TLS close notification, keep-alive and nonblocking outbound TCP |
| Corpus and deterministic mutations | `test_std_actor_http_corpus` passed: 80 shared requests sent in 1-byte and 64-byte writes, plus 600 mutations |
| Coverage-guided fuzzing | Incremental decoder: 1,688,343 libFuzzer executions in 61 seconds, seed 592601, zero findings; coverage instrumentation, not ASan/TSan. Whole actor lifecycle is covered by socket tests rather than this target |
| Slow clients and output back-pressure | Socket tests passed for partial heads/bodies, handshakes and slow readers |
| Shutdown under load and signals | Forced drain with a busy mailbox passed; SIGTERM completes an in-flight streamed response; HTTP/TLS application shutdown is tested, including a TLS peer omitting close notification |
| Repeated failure/resource stability | Repeated handler traps, heap exhaustion, factory failures and 5,120 connections at concurrency 16 pass; issue #641 remains for fresh TLS configurations retained after traps |
| TLS and reverse proxies | curl 8.18.0, nginx 1.30.6 and Caddy 2.11.7 passed streamed/sized/Expect uploads, chunked replies, keep-alive, repeated 413 refusals and proxy TLS verification |
| Installed/relocated toolchain | Actor application qualification pending |
| Comparative measurements | Shared harness available; qualified actor measurements pending |

This table records evidence rather than inheriting a pass from a shared
component. The [B85 backlog entry](../../backlog.md) owns remaining work.
The [design](../../proposals/actor-http-server.md) records the two-server goal
and acceptance rationale. Update the evidence with actual commands, revisions,
platform, limits and results before changing the readiness label.

The actor-specific suites are in `tests/test_std_actor_http_server.rs` and
`tests/test_std_actor_http_corpus.rs`; decoder tests are in
`tests/test_std_http_decoder.rs`. Run the integrated suites with
`cargo test --locked --all-features --test integration test_std_actor_http_`
and the decoder with the `test_std_http_decoder::` filter. These results do
not replace the remaining gates above.

## Performance comparison

`examples/http_server_bench.py` selects the worker or actor implementation,
with corresponding plain and TLS fixtures. It uses the same small response,
16 KiB response, POST echo, keep-alive and new-connection workloads. Compiler
hash, revision, source, CPU affinity, settings, failures, throughput, latency,
CPU time and memory are recorded with the results. Failed or incomplete
requests make the driver fail; they are not a clean throughput result.

For an equal-core comparison, use one worker and one actor stage on the same
server CPU, with the client on a separate CPU. Run each at least three times,
alternating implementations against fresh processes. Compare the ordinary
four-worker configuration separately; it has a different CPU budget. Match
idle connection counts explicitly (`--idle 984` fits both), and keep load-test
concurrency within both admission limits. Use `--tls` for HTTPS.

Performance under faults is a separate measurement: combine healthy traffic
with trapped handlers, heap exhaustion, slow clients and overload, and record
healthy-request latency, recovery, lost connections and resource growth.
Clean-request throughput alone does not measure the actor server's purpose.
Machine load and client bottlenecks must accompany any reported comparison.
