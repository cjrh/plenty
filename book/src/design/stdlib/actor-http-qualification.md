# Actor HTTP qualification

This record covers [`std.actor.http.server`](actor-http-server.md).
Its core HTTP/TLS implementation has passed the checks recorded below.
**It is not yet labelled production-ready:** the platform/sanitizer scope
below is deliberately limited. TLS configuration retention after traps (issue #641)
is addressed by [heap-local reconstruction](actor-tls.md#memory). The [worker server's qualification](http-qualification.md)
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
| Repeated failure/resource stability | Repeated handler traps, heap exhaustion, factory failures and 5,120 connections at concurrency 16 pass; fresh-configuration trap/normal-drop/construction-OOM regressions check actor heaps and process allocation balance |
| TLS and reverse proxies | curl 8.18.0, nginx 1.30.6 and Caddy 2.11.7 passed streamed/sized/Expect uploads, chunked replies, keep-alive, repeated 413 refusals and proxy TLS verification |
| Installed/relocated toolchain | Copied compiler alone into an empty environment, compiled HTTP/TLS applications, removed compiler and source, then ran both native binaries through body/echo/trap/recovery/shutdown probes successfully |
| Comparative measurements | Three alternating runs of all six workloads for HTTP/TLS, actor/one-worker/four-worker: 540,000 measured requests, zero failures; mixed-fault comparison also passed for the actor |

This table records evidence rather than inheriting a pass from a shared
component. The [B85 backlog entry](../../backlog.md) owns remaining work.
The [design](../../proposals/actor-http-server.md) records the two-server goal
and acceptance rationale. The remaining platform scope is x86-64 GNU/Linux; no macOS, ARM, ASan or
TSan result is implied by these checks.

The actor-specific suites are in `tests/test_std_actor_http_server.rs` and
`tests/test_std_actor_http_corpus.rs`; decoder tests are in
`tests/test_std_http_decoder.rs`. Run the integrated suites with
`cargo test --locked --all-features --test integration test_std_actor_http_`
and the decoder with the `test_std_http_decoder::` filter. These results do
not establish behavior on untested platforms.

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

## Recorded comparison

These measurements predate the heap-local TLS configuration reconstruction for
issue #641. Their source/compiler provenance and results remain unchanged. They
do not establish current TLS setup costs or memory use; actor sessions now own a
parsed configuration and rebuild their keys and verification state on construction.
Plain HTTP does not use that path.


Measured on 2026-11-01 at source revision `12c021c9`, with compiler SHA-256
`53e6b1c791bd104ba9795a739b91211025d2b10201ed757ca0e053cfc5ccc8d4`.
Native fixtures were compiled with `--release`, on a Ryzen 7 7840HS,
Linux 7.2.11 x86-64. One-worker and actor servers used CPU 0; four workers used
CPUs 0, 2, 4 and 6 (four physical cores). Clients used separate physical cores
8, 10, 12 and 14. No compilation, qualification suite or fault load ran alongside
these measurements. Machine load snapshots are retained in the
[raw results and reproduction sources](../../assets/actor-http-comparison.json).

Each configuration ran three times in alternating worker/actor order against
fresh processes: 5,000 requests per workload per run, with 984 connections for
the separate idle measurement. These are short loopback measurements, not
sustained deployment-capacity claims. ApacheBench measured ordinary workloads;
four curl processes measured new TLS connections. The result artifact contains
all sample ranges, p50/p99 latency, server CPU time, RSS, failures and settings.

Median requests/second (all measured requests completed successfully):

### HTTP

| Workload | One worker / one core | Actor / one core | Four workers / four cores |
|---|---:|---:|---:|
| Small, 1 keep-alive client | 21,779 | 30,078 | 23,433 |
| Small, 16 keep-alive clients | 25,545 | 27,703 | 96,802 |
| Small, 64 keep-alive clients | 24,515 | 13,597 | 82,129 |
| New connection, 16 clients | 15,160 | 12,215 | 15,035 |
| 16 KiB response, 16 clients | 18,575 | 13,402 | 72,903 |
| 1 KiB POST echo, 16 clients | 24,215 | 25,071 | 98,404 |

### HTTPS

| Workload | One worker / one core | Actor / one core | Four workers / four cores |
|---|---:|---:|---:|
| Small, 1 keep-alive client | 19,195 | 26,605 | 21,324 |
| Small, 16 keep-alive clients | 22,568 | 25,430 | 79,376 |
| Small, 64 keep-alive clients | 20,663 | 11,976 | 65,670 |
| New connection, 16 clients | 3,796 | 4,109 | 4,285 |
| 16 KiB response, 16 clients | 14,902 | 11,859 | 53,654 |
| 1 KiB POST echo, 16 clients | 20,939 | 23,104 | 81,723 |

The actor wins several small-response cases on one core, while the worker
wins at 64 clients and for 16 KiB responses. Four worker cores provide much
higher concurrent throughput. HTTP p99 at 16 small-response clients was
1.013 ms for one worker, 1.625 ms for the actor, and 0.286 ms for four workers;
a throughput improvement need not improve tail latency.

Median incremental idle memory per connection was 0.86 KiB (one worker),
21.73 KiB (actor), and 1.13 KiB (four workers) for HTTP. With TLS it was
41.07, 45.86 and 41.35 KiB respectively. These are resident-memory differences
for this fixture, not the configured maximum actor heap reservation.

The 64-client actor HTTP samples recorded roughly 617,000 full-mailbox retry
attempts for 5,000 completed responses, without lost messages or failed HTTP
requests. Issue #644 tracks this measured scheduling/accounting overhead.
The raw actor statistics also distinguish abrupt TLS peer closure from an
HTTP request failure: curl/ApacheBench can receive the complete response and
close without TLS close notification.

Reproduce a single configuration from the repository root; change
`--implementation actor` to `--implementation worker --workers 1`, or use
`--implementation worker --workers 4 --server-cpus 0,2,4,6`, and add `--tls`
for HTTPS:

```sh
python3 examples/http_server_bench.py --compiler target/debug/plenty \
    --implementation actor --server-cpus 0 --client-cpus 8,10,12,14 \
    --requests 5000 --idle 984 --runs 3 --output target/actor-http-comparison
```

For the recorded samples, each invocation used `--runs 1` and the six
configurations alternated for three rounds. Adapt CPU affinity to the machine.

## Mixed failures

The separate fault workload placed 32 stalled request heads beside four
healthy clients and 100 intentional handler traps in each of four batches.
All 4,000 healthy requests and 400 post-batch recovery requests succeeded on
the actor server; all 400 trapping connections ended without an HTTP response.
RSS stayed at 4,136 KiB and descriptor count at six after every batch.
The server then answered its shutdown request and exited successfully, with
zero rejected admissions or lost messages. Healthy batch p99 was 1.20–1.28 ms.

Both worker configurations passed their ordinary and stalled-head baselines,
then exited on the first intentional runtime trap, as their execution contract
specifies. This comparison demonstrates trap containment, not immunity to
arbitrary native faults. CPU 6 and client CPU 7 are SMT siblings in this
separate probe, so its latency is descriptive failure evidence rather than a
throughput comparison. The result artifact embeds the temporary fault harness
and its two fixtures, along with compiler and native-binary hashes.
