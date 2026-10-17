# HTTP server qualification

This page records what has been checked before [`std.http.server`](http-server.md)
could be called production-ready, what that showed, and how to run the
server in the meantime. **The server is not yet production-ready.** No
gate fails, but some are only partly covered; see [Gates](#gates). HTTP/2 and HTTP/3 need
separate adoption decisions and are not part of these gates.

The suites are in `tests/test_std_http_server_qualification.rs`, with the
request corpus in `tests/fixtures/http_server/`. The benchmark is
`examples/http_server_bench.py`. Everything runs on loopback with no network.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| Parser conformance and a retained corpus | Pass | About 80 corpus requests, each replayed in 64-byte reads and one byte at a time; 600 mutated copies. Includes the RFC 9112 section 3.2 `Host` rules and absolute-form targets |
| Fuzzing | Partial | Deterministic mutation of the corpus through the whole connection loop, here and in `test_std_http.rs`; no coverage-guided fuzzing |
| Interoperability | Pass | curl over plain HTTP and TLS (reuse, sized, chunked and `100-continue` uploads, HTTP/1.0, `HEAD`, chunked and 64 MiB responses, 404/405); `std.http.client` (pooling, streamed chunked upload, large downloads) |
| Slow clients | Pass | Idle and silent clients, partial request heads, partial TLS handshakes, slow readers and slow body senders are cut off at their deadlines without holding the server; heads and handshakes wait parked, not on a worker. See [Slow request heads](#slow-request-heads) |
| Refusals without a reset | Pass | Refusals of requests not read to the end (`431`, `413`, `400`, a failed body read), each followed by more input, end with the whole answer and a clean end, over plain TCP (parked and on a worker) and TLS (`close_notify`). See [Refusals and resets](#refusals-and-resets) |
| Shutdown under sustained load | Pass | Six clients sending keep-alive requests while shutdown begins: every response received is whole, the server's count matches, and `serve` returns within the drain deadline |
| Supervisor stop | Pass | With [`std.signal.cancel_on`](signal.md#stopping-a-server), `SIGTERM` during a streamed response begins shutdown: the response ends whole, `serve` returns its `Report` and the program exits 0. A second signal ends the program at once (`test_std_signal.rs`) |
| Allocation failure | Pass | Every allocation on the accepting thread fails in turn; request handling fails in turn through `serve_connection` (`test_std_http_server.rs`). No trap, no leak, and the request is served once allocation succeeds |
| Resource leaks | Pass | No descriptor survives a load round or a fault round (bad heads, truncated heads, silent and rejected clients), and the allocation-checks runtime finds no live heap after either |
| Measured baselines | Published | [Baselines](#baselines) |
| Unused backends | Pass | A JSON service using the server and client links none of rustls/ring, SQLite or zlib-rs |
| Relocated offline toolchain | Partial | The compiler binary, copied alone into a fresh directory and run with an empty environment except `PATH`, builds and runs a JSON service. An installed package workflow (`plenty build --offline` from an installed toolchain) and the CLI/data workflow are not covered |
| TLS and proxy deployment | Partial | Direct TLS is tested with curl and `std.tls` clients. Reverse-proxy deployment is documented below but not tested against a real proxy |

## Supported platforms

x86-64 Linux with the GNU C library, the only target of
[`std.net`](net.md) and [`std.ready`](ready.md) (epoll). Results here were
taken on Linux 7.2 (Fedora 44). No other target is supported.

## Handler errors and traps

A handler reports a problem by returning `Err`; the server answers it, counts
it and goes on serving (see [When a handler fails](http-server.md#when-a-handler-fails)).
A fatal runtime error is different. An index out of range, integer overflow,
division by zero, `unwrap` of an `Err`, or worker stack exhaustion terminates
the whole process at once, without unwinding, with a message such as
`error: main.plenty:3:11: index out of bounds: index 7, length 3` and exit
status 1. Every connection is cut off and no `Report` is returned.

So in a handler:

- Check indexes and arithmetic on anything that comes from a request, use
  optional lookups and `?`, and return `Err` rather than calling `unwrap`.
- Run the server under a supervisor that restarts it (systemd
  `Restart=on-failure`, a container restart policy).
- Allocation failure is not a trap: it closes the affected connection.

## Baselines

`examples/http_server_bench.py` builds `examples/http_server_bench.plenty`
with `--release`, runs it with 4 workers on CPUs 0, 2, 4 and 6 and
ApacheBench 2.3 on CPUs 8, 10, 12 and 14 (one hardware thread per physical
core), and repeats each workload against a fresh server.

Measured on 2026-10-17 at revision `59803fb` plus this change, built
by rustc 1.99.0 (target `x86_64-unknown-linux-gnu (CPU baseline: x86-64/SSE2)`), on an AMD Ryzen 7 7840HS w/ Radeon 780M Graphics with
Linux 7.2.9-201.fc44.x86_64, three runs per workload. Other builds were running on
the machine (load average about 8), so treat the ranges as noise bounds.

| Workload | Requests/s, median (range) | p50 / p99 latency, ms | Peak RSS, KiB | Failed |
|---|---:|---:|---:|---:|
| keep-alive, 1 client | 3785 (2946–4423) | 0.23 / 1.28 | 5556 | 0 |
| keep-alive, 16 clients | 15371 (15216–17494) | 1.03 / 3.33 | 5624 | 0 |
| keep-alive, 64 clients | 16744 (15261–16952) | 3.96 / 7.22 | 5592 | 0 |
| new connection per request, 16 clients | 9239 (7129–9485) | 1.65 / 3.70 | 5620 | 0 |
| keep-alive 16 KiB response, 16 clients | 13199 (12749–13359) | 1.31 / 4.37 | 5632 | 0 |
| keep-alive 1 KiB POST echo, 16 clients | 11904 (11425–11952) | 1.33 / 3.69 | 5640 | 0 |

With 1000 parked keep-alive connections the server's resident
memory grew from 4392 KiB to 5828 KiB, about
1.44 KiB per idle connection.

Latency is per request as ApacheBench sees it. Peak RSS is the server's
`VmHWM`. `16k` is a 16 KiB response; the POST echoes a 1 KiB body. Each
keep-alive connection closes after `max_requests` (1000) requests, so
ApacheBench reconnects.

Limits of these numbers: one machine, loopback only, a client that is
itself a bottleneck at high rates, small fixed responses, and no TLS. They
are a regression baseline, not a capacity plan.

Before this qualification, a response whose head and body went out as two
writes (any body over the 4 KiB write buffer) waited about 40 ms for a
delayed acknowledgement: the 16 KiB workload ran at 388 requests/s. The
server now turns off Nagle's algorithm on accepted connections.

## Slow request heads

Until issue #132 was fixed, a worker read each request head as it arrived,
so a client that sent part of a head held a worker until `head_timeout`.
Now a partial head, like a partial TLS handshake, waits on the accepting
thread (see [Idle connections](http-server.md#idle-connections)).
Measured on 2026-10-17 with the scenario of `slow_heads_do_not_hold_workers`
below, also run with the defaults: clients that each send
`GET / HTTP/1.1\r\nHost: x` and stall, then one complete request.

| Configuration | Before | After |
|---|---|---|
| 2 workers, `head_timeout` 1 s, 6 slow clients | Answered after 2954 ms | Answered within 2 ms |
| `config()` defaults (4 workers, 10 s), 4 slow clients | Answered after 9959 ms | Answered within 2 ms |
| `config()` defaults, 64 slow clients | `503` at once; 44 slow clients got `503`, the last `408` came after 50 s | Answered within 2 ms; all 64 got `408` at 10 s |

The slow clients still count against `idle_connections` (default 256):
with that many parked, new connections get `503` until their deadlines
pass. With 1000 parked partial heads the server's resident memory grew
from 4548 KiB to 6396 KiB, about 1.9 KiB per connection. Size
`idle_connections` and the descriptor limit for the slow clients an exposed
server must outlast. Keep-alive and new-connection throughput showed no
change beyond run-to-run noise on a loaded machine (load average about 8).
`slow_heads_do_not_hold_workers` in the qualification suite and
`partial_heads_wait_without_a_worker` and
`https_partial_handshakes_and_heads_wait_without_a_worker` in
`test_std_http_server.rs` check this.

## Refusals and resets

Until issue #138 was fixed, a refusal of a request the server had not read
to the end closed the socket with request bytes unread, so Linux reset the
connection. Measured on 2026-10-17 against
`examples/http_server_bench.plenty`, ten connections per case, each
sending the request and then reading to the end:

| Request | Before | After |
|---|---|---|
| Head with a 20000-byte field (`431`) | 10 resets | 10 clean ends |
| `Content-Length` over `max_body` and 200 KB of body (`413`) | 10 resets | 10 clean ends |
| Malformed field line and 100 KB more (`400`) | 10 resets | 10 clean ends |
| Bad chunk size and 100 KB more (`400` from a failed body read) | 10 resets | 10 clean ends |

The results were the same with `IDLE=0`, where the worker lingers. Before
the fix a client reading to the end saw `ECONNRESET`, sometimes after
reading the answer. Now the
server ends its output, discards up to 64 KiB for up to 2 s, then closes
(see [Lingering close](http-server.md#lingering-close)).
`refusals_end_cleanly_instead_of_resetting` and
`tls_refusals_end_with_close_notify` in the qualification suite check this.

## Deployment

For a public service, put a reverse proxy in front of the server (see
[deploying behind a reverse proxy](http-proxy.md#deploying-behind-a-reverse-proxy)).
The proxy also absorbs slow clients beyond what `idle_connections` bounds:

- Buffer whole request heads, and preferably bodies, before forwarding
  (nginx does both by default; keep `proxy_request_buffering on`).
- Connect to the server over loopback or a private network with
  `proxy_http_version 1.1` and keep-alive, so connections are reused.
- Set the proxy's client timeouts and connection limits per client; the
  server's limits are per connection.

The server itself:

- Listen on loopback or a private address. Serve
  [HTTPS directly](http-proxy.md#serving-https-directly) only when no proxy
  is possible, and then keep `head_timeout` and `handshake_timeout` short
  and `idle_connections` above the number of slow clients expected.
- Raise the descriptor limit (`LimitNOFILE` in systemd) above
  `idle_connections + queue + workers` plus the program's own files; when it
  runs out, `accept` fails, is counted in `accept_errors` and retried every
  10 ms.
- Set `events` to a [`std.log`](log.md) channel sink and export the
  `Report` counts; `rejected`, `timeouts` and `accept_errors` show overload.
- Drain on `SIGTERM` with [`std.signal`](signal.md#stopping-a-server), and
  set the supervisor's stop timeout (`TimeoutStopSec` in systemd) above
  `drain_timeout`.

## Security fixes and backend updates

The HTTP modules are checked Plenty in the compiler's embedded standard
library; a fix reaches a program when it is rebuilt with a fixed compiler.
For a defect in parsing, framing or limits:

1. Record it in the issue register with severity `high`, and add the
   triggering request to `tests/fixtures/http_server/corpus.txt` with its
   correct answer, so it stays fixed.
2. Fix it in `stdlib/std/http.plenty` or `stdlib/std/http/`, and run this
   suite with `test_std_http.rs`, `test_std_http_server.rs`,
   `test_std_http_client.rs` and `test_std_http_helpers.rs`.
3. Rebuild and redeploy every program that serves HTTP.

TLS uses vendored rustls and ring, pinned in `Cargo.lock`. Follow rustls and
ring advisories and update them as described under
[TLS supply](tls.md#supply), then run `test_std_tls.rs`,
`test_std_http_https.rs` and the TLS tests here. No other native backend is
linked into a program that only serves HTTP.

## Reproducing

```text
cargo test --locked --all-features --test test_std_http_server_qualification
cargo build --release --locked
python3 examples/http_server_bench.py --compiler target/release/plenty --output target/http-bench --runs 3
```

The curl tests are skipped when curl is not installed; the benchmark needs
ApacheBench (`ab`) and `taskset`. Busy machines distort the benchmark: check
the load average first, and keep the total number of busy threads at or
below the number of physical cores.
