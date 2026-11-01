# Actor HTTP TLS comparison

Current matched measurements for [`std.http.server`](http-server.md) and
[`std.actor.http.server`](actor-http-server.md). The
[actor qualification record](actor-http-qualification.md) retains the original
comparison and the platform limits.

Measured on 2026-11-01 at source revision `c397a845`, after the private TLS
configuration reconstruction (issue #641) and completion-credit changes
(issue #644). The [raw results and replay sources](../../assets/actor-https-comparison.json)
retain all samples, source fixtures, the driver, a reproduction script, an
independent validation script, compiler/native hashes and machine load snapshots.
Compiler SHA-256:
`13c6ed9514305a6d67592417265034555283bb7e8635188dcddc77091c907aa6`.
The compiler was built with `cargo build --locked --bin plenty`; native fixtures
used `--release --compile` with `PLENTY_CACHE=off`.

The Ryzen 7 7840HS host ran Linux 7.2.11 x86-64 GNU/Linux. One worker and the
single actor stage used CPU 0; four workers used physical cores 0, 2, 4 and 6.
Clients used physical cores 8, 10, 12 and 14. Three rounds alternated
worker/actor/four-worker order, reversing it in the middle round. Every workload
started a fresh server, with 5,000 requests at the stated concurrency. All
270,000 measured requests completed with zero failures or missing responses.
Idle measurements used 984 connections in each configuration. No task-local
builds or search indexing ran during measurement; unrelated shared-host activity
was uncontrolled. The host's one-minute load average was 2.88 before and 2.72
after the complete run.

## HTTPS throughput and latency

Median requests/second across three samples:

| Workload | One worker / one core | Actor / one core | Four workers / four cores |
|---|---:|---:|---:|
| Small, 1 keep-alive client | 18,876 | 25,956 | 20,925 |
| Small, 16 keep-alive clients | 21,874 | 34,379 | 89,255 |
| Small, 64 keep-alive clients | 19,955 | 28,641 | 64,121 |
| New connection, 16 clients | 3,788 | 3,794 | 4,465 |
| 16 KiB response, 16 clients | 14,279 | 13,332 | 51,283 |
| 1 KiB POST echo, 16 clients | 21,051 | 30,171 | 84,747 |

The actor leads the equal-core small-response and echo cases in these samples;
one worker leads the 16 KiB case, and four worker cores have substantially higher
concurrent throughput. New-connection throughput is nearly equal between the
single-core configurations. This is a current comparison of the implementations,
not an isolated before/after measurement of either fix.

Median sample p99 latency in milliseconds (new-connection latency includes TLS
and the HTTP response):

| Workload | One worker | Actor | Four workers |
|---|---:|---:|---:|
| Small, 1 keep-alive client | 0.092 | 0.064 | 0.085 |
| Small, 16 keep-alive clients | 1.576 | 0.579 | 0.443 |
| Small, 64 keep-alive clients | 28.137 | 22.583 | 19.484 |
| New connection, 16 clients | 5.313 | 5.394 | 4.324 |
| 16 KiB response, 16 clients | 1.817 | 1.441 | 0.575 |
| 1 KiB POST echo, 16 clients | 0.981 | 0.629 | 0.367 |

The 64-client p99 values are high for all configurations. The three p99 samples
range from 24.349–32.037 ms for one worker, 22.431–22.710 ms for the actor,
and 16.729–24.064 ms for four workers. Short loopback runs on a shared host do
not establish a service latency objective or sustained deployment capacity.
ApacheBench drives keep-alive workloads; four parallel curl processes drive new
connections, without certificate verification, as in the historical comparison.

## TLS handshake and idle memory

A separate Python probe made 1,000 new connections at concurrency 16 per sample,
three samples per configuration: 9,000 successful verified TLS handshakes and
validated HTTP responses. It used one shared client SSL context loaded with the
fixture CA. Every session negotiated TLS 1.3 with `TLS_AES_256_GCM_SHA384` and
reported no session reuse. Timing begins after TCP connect and ends when the
client's TLS handshake returns, before the HTTP request. It includes client
verification, scheduling and library overhead; it does not isolate server CPU
cost or prove that the server has consumed the client's Finished message.

| Measurement | One worker | Actor | Four workers |
|---|---:|---:|---:|
| Median sample handshake p50, ms | 1.782 | 3.630 | 1.383 |
| Median sample handshake p99, ms | 3.489 | 5.443 | 5.192 |
| Median incremental idle RSS, KiB/connection | 41.12 | 57.80 | 41.42 |

The actor fixture uses 16.68 KiB more incremental idle RSS than one worker,
including its private per-session configuration, for this ECDSA P-256 certificate
fixture. This comparison does not isolate configuration memory from other actor
overhead. Idle RSS is a process resident-memory difference
with connection capacity raised to 1,000, not an actor heap high-water mark or
a general heap-sizing recommendation. The [historical 45.86 KiB actor TLS figure](actor-http-qualification.md#recorded-comparison)
predates the ownership change. Different certificate chains, root sets,
record sizes and application state need their own measurement.

## TLS failures and recovery

A separate descriptive probe combined 32 stalled TLS request heads, four healthy
clients and 100 intentional handler traps per batch. Across four actor batches,
all 4,000 healthy requests and 400 post-batch recovery requests succeeded;
all 400 trap requests ended without a successful response. Healthy-request p99,
including TLS setup, ranged from 1.699 to 2.957 ms. After each batch RSS was
5,128 KiB and the descriptor count was six; before fault injection RSS was
5,124 KiB. Shutdown succeeded with exactly 4,501 completed requests (including
100 baseline requests and `/stop`), 400 trapped actors, zero rejected admissions
and zero lost messages. Abrupt client TLS closes also increment actor failure
counts; they do not mean those complete HTTP responses failed.

Both worker configurations completed their 100-request baselines, then exited
with status 1 on the first handler trap; neither served recovery requests.
This demonstrates the actor containment contract for these injected traps.
It is one fault run per configuration, with a fixed root TLS configuration,
not a configuration-rotation soak, an allocation-failure test, or a new platform
qualification. The actor server's production-readiness limits remain unchanged.

Replay from the recorded source revision with ApacheBench, curl, taskset and a
compiler built there. Extract the scripts from this artifact before switching
to that checkout, then pass its path and compiler to the replay script:

```sh
python3 - <<'PYREPLAY'
import json
from pathlib import Path
record = json.loads(Path("book/src/assets/actor-https-comparison.json").read_text())
Path("/tmp/actor-https-replay.py").write_text(record["reproduction_script"])
Path("/tmp/actor-https-validate.py").write_text(record["validation_script"])
PYREPLAY
TLS_CHECKOUT=/path/to/checkout-of-c397a845
python3 /tmp/actor-https-replay.py "$TLS_CHECKOUT" \
    "$TLS_CHECKOUT/target/debug/plenty" /tmp/actor-https-replayed
python3 /tmp/actor-https-validate.py /tmp/actor-https-replayed/results.json
```

Adapt CPU affinities to the host. Keep newly recorded results separate from the
published artifact; numerical RSS stability and negotiated-cipher assertions in
the validation script describe this fixture and platform, and may need adjustment
elsewhere.
