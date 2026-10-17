# HTTP server qualification

This page records what has been checked before [`std.http.server`](http-server.md)
could be called production-ready, what that showed, and how to run the
server in the meantime. **The server is not production-ready.** Two gates
fail (slow request heads, issue #132; termination signals, issue #133) and
some are only partly covered; see [Gates](#gates). HTTP/2 and HTTP/3 need
separate adoption decisions and are not part of these gates.

The suites are in `tests/test_std_http_server_qualification.rs`, with the
request corpus in `tests/fixtures/http_server/`. The benchmark is
`examples/http_server_bench.py`. Everything runs on loopback with no network.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| Parser conformance and a retained corpus | Pass, with known deviations | About 60 corpus requests, each replayed in 64-byte reads and one byte at a time; 600 mutated copies. Deviations: `Host` is not checked and absolute-form targets get `400` (issue #131) |
| Fuzzing | Partial | Deterministic mutation of the corpus through the whole connection loop, here and in `test_std_http.rs`; no coverage-guided fuzzing |
| Interoperability | Pass | curl over plain HTTP and TLS (reuse, sized, chunked and `100-continue` uploads, HTTP/1.0, `HEAD`, chunked and 64 MiB responses, 404/405); `std.http.client` (pooling, streamed chunked upload, large downloads) |
| Slow clients | **Fail** | Idle and silent clients, slow readers and slow body senders are cut off at their deadlines without holding the server. A client that sends part of a head holds a worker until `head_timeout`, so a few of them stall the server (issue #132) |
| Shutdown under sustained load | Pass | Six clients sending keep-alive requests while shutdown begins: every response received is whole, the server's count matches, and `serve` returns within the drain deadline |
| Supervisor stop | **Fail** | A program cannot receive `SIGTERM`, so it cannot drain when stopped (issue #133) |
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

## Deployment

For a public service, put a reverse proxy in front of the server (see
[deploying behind a reverse proxy](http-proxy.md#deploying-behind-a-reverse-proxy)).
Until issue #132 is fixed, the proxy must also absorb slow clients:

- Buffer whole request heads, and preferably bodies, before forwarding
  (nginx does both by default; keep `proxy_request_buffering on`).
- Connect to the server over loopback or a private network with
  `proxy_http_version 1.1` and keep-alive, so connections are reused.
- Set the proxy's client timeouts and connection limits per client; the
  server's limits are per connection.

The server itself:

- Listen on loopback or a private address. Serve
  [HTTPS directly](http-proxy.md#serving-https-directly) only when no proxy
  is possible, and then keep `head_timeout` short and `workers` above the
  number of slow clients expected.
- Raise the descriptor limit (`LimitNOFILE` in systemd) above
  `idle_connections + queue + workers` plus the program's own files; when it
  runs out, `accept` fails, is counted in `accept_errors` and retried every
  10 ms.
- Set `events` to a [`std.log`](log.md) channel sink and export the
  `Report` counts; `rejected`, `timeouts` and `accept_errors` show overload.
- Stop the server in-band until issue #133 is fixed: cancel the `stop` token
  from a route bound to loopback only, or accept that a signal ends it
  without a drain.

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
