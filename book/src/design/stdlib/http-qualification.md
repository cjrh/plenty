# HTTP server qualification

This page records what was checked before [`std.http.server`](http-server.md)
was labelled production-ready, what that showed, and how to deploy it.
**The server is production-ready:** every gate passes; see [Gates](#gates). HTTP/2 and HTTP/3 need
separate adoption decisions and are not part of these gates.

The suites are in `tests/test_std_http_server_qualification.rs`, with the
request corpus in `tests/fixtures/http_server/`, and in
`tests/test_std_http_server_proxy.rs`, which runs the server behind nginx and
Caddy in Docker (see [Reverse proxies](#reverse-proxies)). The benchmark is
`examples/http_server_bench.py`. Everything runs on loopback; only the proxy
tests need a network, once, to pull their images.

## Gates

| Gate | Status | Evidence |
|---|---|---|
| Parser conformance and a retained corpus | Pass | About 80 corpus requests, each replayed in 64-byte reads and one byte at a time; 600 mutated copies. Includes the RFC 9112 section 3.2 `Host` rules and absolute-form targets |
| Fuzzing | Pass | Coverage-guided libFuzzer runs of the whole connection loop, seeded from the corpus, with every response status checked: 16.9 million inputs in 30 minutes, no finding. A short run is part of the test suite (`test_fuzz.rs`), beside deterministic corpus mutations here and in `test_std_http.rs`. See [Fuzzing](#fuzzing) |
| Interoperability | Pass | curl over plain HTTP and TLS (reuse, sized, chunked and `100-continue` uploads, HTTP/1.0, `HEAD`, chunked and 64 MiB responses, 404/405); `std.http.client` (pooling, streamed chunked upload, large downloads) |
| Slow clients | Pass | Idle and silent clients, partial request heads, partial TLS handshakes, slow readers and slow body senders are cut off at their deadlines without holding the server; heads and handshakes wait parked, not on a worker. See [Slow request heads](#slow-request-heads) |
| Refusals without a reset | Pass | Refusals of requests not read to the end (`431`, `413`, `400`, a failed body read), each followed by more input, end with the whole answer and a clean end, over plain TCP (parked and on a worker) and TLS (`close_notify`). See [Refusals and resets](#refusals-and-resets) |
| Shutdown under sustained load | Pass | Six clients sending keep-alive requests while shutdown begins, each reusing its connection, with responses still arriving after the stop signal: every response received is whole, the server's count matches, and `serve` returns within the drain deadline |
| Supervisor stop | Pass | With [`std.signal.cancel_on`](signal.md#stopping-a-server), `SIGTERM` during a streamed response begins shutdown: the response ends whole, `serve` returns its `Report` and the program exits 0. A second signal ends the program at once (`test_std_signal.rs`) |
| Allocation failure | Pass | Every allocation on the accepting thread fails in turn; request handling fails in turn through `serve_connection` (`test_std_http_server.rs`). No trap, no leak, and the request is served once allocation succeeds |
| Resource leaks | Pass | No descriptor survives a load round or a fault round (bad heads, truncated heads, silent and rejected clients), and the allocation-checks runtime finds no live heap after either |
| Measured baselines | Published | [Baselines](#baselines), over plain HTTP and HTTPS |
| Unused backends | Pass | A JSON service using the server and client links none of rustls/ring, SQLite or zlib-rs |
| Relocated offline toolchain | Pass | The compiler binary, copied alone into a fresh directory and run with an empty environment except `PATH`, builds and runs a JSON service. Installed alone as `PREFIX/bin/plenty` with a fresh home, it fetches a package's Git dependency once; with the repository deleted and `git` replaced by a failing stub, `plenty build --offline --locked` and `plenty run --offline` build a CLI/data tool (`std.cli`, `std.csv`, `std.json`, typed `std.error` reports and exit statuses) and a JSON service over the same dependency. Both run and link no unused backend |
| TLS and proxy deployment | Pass | Direct TLS is tested with curl and `std.tls` clients. Behind nginx 1.30.6 and Caddy 2.11.7: keep-alive on both sides of the proxy, buffered and streamed bodies, `100-continue`, forwarded `Host` and `X-Forwarded-*`, TLS terminated at the proxy, slow heads, a slow upstream, refusals and a drain on `SIGTERM`. Opt-in locally (`PLENTY_DOCKER_TESTS=1`) and run by CI; see [Reverse proxies](#reverse-proxies) |

## Supported platforms

x86-64 Linux with the GNU C library, the only target of
[`std.net`](net.md) and [`std.ready`](ready.md) (epoll). Results here were
taken on Linux 7.2 (Fedora 44). No other target is supported.

## Application error composition

The installed offline package test also checks B14/B71 through its shared CSV
summary dependency. The CLI reports invalid integer data with its record number
and `ParseError`, malformed row width with the complete `CsvError`, and a missing
file with its nested stream/I/O cause. Each report retains the operation and path,
prints no partial JSON, and exits 1; argument errors still exit 2 and help exits 0.

The JSON service returns those same causes as `Context[SummaryError]` in its
handler error sum and reports them before propagation. One client repeats all
three failures and a successful summary three times. The test checks identical
CLI/service diagnostics, generic `500` responses without private error details,
successful requests after failures, nine handler errors among twelve requests,
and no protocol errors. After the client, workers, CSV readers and listener have
dropped, the process has the same number of open descriptors as before the run.
This qualifies application composition over the existing CSV/HTTP APIs; typed
chunked framing, validated heads and full client cause preservation remain
separate work in [B71](../../backlog.md).

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
core, so at most ten threads are busy), and repeats each workload against a
fresh server. With `--tls` it builds `examples/https_server_bench.plenty`
instead, the same handler served by `std.http.tls` with the ECDSA P-256
certificate in `tests/fixtures/tls`.

Measured on 2026-10-18 at revision `32c48c0`, built by rustc 1.99.0 (target
`x86_64-unknown-linux-gnu (CPU baseline: x86-64/SSE2)`), on an AMD Ryzen 7
7840HS w/ Radeon 780M Graphics with Linux 7.2.9-201.fc44.x86_64, three runs
per workload. Other builds were running (load average about 4.5 at the
start of each run), so treat the ranges as noise bounds.

| Workload | Requests/s, median (range) | p50 / p99 latency, ms | Peak RSS, KiB | Failed |
|---|---:|---:|---:|---:|
| keep-alive, 1 client | 5560 (5536–5662) | 0.17 / 0.36 | 6212 | 0 |
| keep-alive, 16 clients | 33314 (32772–33388) | 0.10 / 2.96 | 6212 | 0 |
| keep-alive, 64 clients | 32853 (29653–32959) | 0.10 / 9.03 | 6236 | 0 |
| new connection per request, 16 clients | 16106 (16085–16137) | 0.93 / 1.64 | 6212 | 0 |
| keep-alive 16 KiB response, 16 clients | 24430 (24323–25589) | 0.14 / 4.30 | 6152 | 0 |
| keep-alive 1 KiB POST echo, 16 clients | 25850 (25783–25897) | 0.16 / 1.84 | 6160 | 0 |

### HTTPS

The same workloads over TLS 1.3 (`TLS_AES_256_GCM_SHA384`, X25519, ECDSA
P-256 server key; rustls 0.23.46 with ring), measured in the same session:

| Workload | Requests/s, median (range) | p50 / p99 latency, ms | Peak RSS, KiB | Failed |
|---|---:|---:|---:|---:|
| keep-alive, 1 client | 3891 (3872–3913) | 0.20 / 0.51 | 8196 | 0 |
| keep-alive, 16 clients | 25892 (25038–26697) | 0.13 / 2.05 | 9352 | 0 |
| keep-alive, 64 clients | 24474 (24459–24516) | 3.33 / 6.19 | 11136 | 0 |
| new connection per request, 16 clients (curl) | 4196 (3939–4235) | 1.43 / 3.76 | 8792 | 0 |
| keep-alive 16 KiB response, 16 clients | 17858 (17382–19415) | 0.19 / 3.69 | 9268 | 0 |
| keep-alive 1 KiB POST echo, 16 clients | 19606 (18753–19782) | 0.91 / 1.49 | 9292 | 0 |

Each new HTTPS connection is a full handshake: the server keeps no session
cache and sends no tickets, so clients cannot resume. ApacheBench cannot
measure that workload: it counts connections closed without `close_notify`
as failed. The script runs curl 8.18.0 (OpenSSL 3.5.9) instead: four processes on the
client CPUs, each with `--parallel-max 4`, `TCP_NODELAY` (curl's default)
and no certificate verification. The same curl method against the plain
server gave 21397 (21225–22032) requests/s, p50 / p99 0.67 / 1.05 ms, so the
handshake, not the client, sets the HTTPS rate.

With 1000 parked keep-alive connections the server's resident memory grew
from 4364 KiB to 6680 KiB over plain HTTP, about 2.3 KiB per idle
connection, and from 5928 KiB to 48552 KiB over HTTPS, about 42.6 KiB per
idle connection: each TLS session keeps its rustls buffers and keys. Size
`idle_connections` for HTTPS with that in mind.

Latency is per request as the client sees it. Peak RSS is the server's
`VmHWM`. `16k` is a 16 KiB response; the POST echoes a 1 KiB body. Each
keep-alive connection closes after `max_requests` (1000) requests, so
ApacheBench reconnects. An earlier plain run, on 2026-10-17 at load average
about 8, gave 9239 to 16744 requests/s for the same workloads; the
difference is machine load, not a server change.

Limits of these numbers: one machine, loopback only, a client that is
itself a bottleneck at high rates, small fixed responses, one certificate
type and no TLS session resumption. They are a regression baseline, not a
capacity plan.

Before this qualification, a response whose head and body went out as two
writes (any body over the 4 KiB write buffer) waited about 40 ms for a
delayed acknowledgement: the 16 KiB workload ran at 388 requests/s. The
server now turns off Nagle's algorithm on accepted connections.

Until issue #148 was fixed, a client that leaves Nagle's algorithm on
(ApacheBench, Python's `socket`) waited about 40 ms on each new HTTPS
connection: a TLS 1.3 server sends nothing after the client's `Finished`, so
the client held its first request until the kernel's delayed
acknowledgement. `ab -n 20 -c 1` without `-k` took 42–44 ms per request.
The server now acknowledges the `Finished` at once; on 2026-10-18 the same
client took 0.76 ms per request over 200 requests (unpinned, not the
baseline method above).

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
with that many parked, new plaintext connections get `503` until slots become
available (new HTTPS connections close). Already-admitted partial heads and
handshakes returned by workers can park beyond that admission cap; see the
[bounded overflow and parking-disabled behavior](http-server.md#idle-connections).
With 1000 parked partial heads the server's resident memory grew
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
server ends its output, discards up to 16 MiB (64 KiB until the proxy tests
below) for up to 2 s, then closes
(see [Lingering close](http-server.md#lingering-close)).
`refusals_end_cleanly_instead_of_resetting` and
`tls_refusals_end_with_close_notify` in the qualification suite check this.

## Fuzzing

`fuzz/http_server/main.plenty` is a [libFuzzer target](../toolchain/01-building/fuzzing.md):
`plenty --fuzz` counts every block the compiled program runs, including the
standard library's, and libFuzzer keeps the inputs that reach new ones. Each
input is one client connection served by `serve_connection` over an
in-memory stream. Its first byte selects the read size (1 to 64 bytes),
small limits (a 64-byte line, a 256-byte head, 4 fields, a 16-byte body)
and a limit of two requests per connection; the rest is what the client
sends. The routes are those of the corpus. Response bodies never repeat the
request, so the target finds every response head in what the server wrote
and traps on a status outside 100, 200, 400, 404, 405, 413, 431, 500, 501
and 505. Traps of the server itself, stack exhaustion and hangs over 10 s
are findings too.

`fuzz/http_server/run.py` builds the target with `--release`, seeds it with
every corpus case in four variants (64-byte reads, 7-byte reads, 1-byte reads,
7-byte reads with small limits) and runs it with the HTTP dictionary `fuzz/http_server/http.dict`.
Findings go to `WORK/findings`; the default `WORK` is
`target/fuzz-http-server`. Findings persist across runs: the script lists
only the files the current run added, and exits nonzero only for those or for
a libFuzzer failure.

```text
cargo build --release --locked
python3 fuzz/http_server/run.py --time 1800 --jobs 2
target/fuzz-http-server/http_server_fuzzer FINDING    # replay one finding
```

On 2026-10-18, on the machine of the [baselines](#baselines), two jobs ran
for 30 minutes each (rustc 1.99.0, clang 22.1.8's libFuzzer), starting from
the corpus of an earlier 10-minute run: 16.9 million inputs, about 4,700 per
second per job, with no finding. 3,777 of the program's 13,878 blocks were
reached, nearly all within the first seconds; most of the rest belong to
functions the target never calls, such as `serve` and TLS, which a fuzz
build keeps.

To act on a finding, replay it, reduce it with `-minimize_crash=1 -runs=100000`,
and handle it as a parser defect under
[Security fixes](#security-fixes-and-backend-updates): its request joins the
corpus, and so the seeds.

Not covered: TLS records (rustls and ring are fuzzed upstream), the
`std.ready` parking of real sockets, and timeouts, since the in-memory stream
never waits.

## Reverse proxies

`tests/test_std_http_server_proxy.rs` puts nginx and Caddy, each in its stock
image, in front of the server, and drives them with curl and raw sockets. The
server runs on the host (it links the host's C library, so it does not run on
Alpine); the proxy container uses `--network host` and reaches it on
loopback. The configurations are `tests/fixtures/proxy/nginx.conf` and
`tests/fixtures/proxy/Caddyfile`; the server is
`tests/fixtures/proxy/server.plenty`.

| Image | Digest |
|---|---|
| `nginx:1.30.6-alpine3.24` | `sha256:970364228b1e4f932abc22d321932816782cca8a20b85a77552a7cfe9acfb3b5` |
| `caddy:2.11.7-alpine` | `sha256:d8542f48d34a9cf4e4c11a478865229840e87e4c96ea3f439101f31a5d35f75f` |

Each test runs against both proxies. Clients connect from `127.0.0.2`, so the
handler can tell them from the proxy at `127.0.0.1`.

| Test | Checks |
|---|---|
| `*_forwarding_keep_alive_and_tls` | `exchange.authority` and `Host` keep the client's `Host`; `X-Forwarded-Proto` is `http` or `https`; [`client_of`](http-proxy.md#clients), trusting only `127.0.0.1`, finds the client and ignores a spoofed `X-Forwarded-For`; curl reuses its connection to the proxy; twelve requests reach the server over at most three connections; TLS terminated at the proxy, with a 700 KB upload |
| `*_bodies_and_continue` | Sized, chunked and `Expect: 100-continue` uploads of 700 KB and a 1 MiB chunked download, through a buffering and a streaming location; a client that waits for `100 Continue` before sending; a streamed response reaches the client before the handler ends it |
| `*_slow_clients_timeouts_and_refusals` | A partial request head is closed by the proxy at its 2 s header timeout and never reaches the server; a handler slower than the proxy's 2 s upstream timeout gets `504` and the next request is served; five rounds of `413` refusals of 2 MiB bodies and early answers to 1000 KiB bodies the handler does not read, sized and chunked, buffered and streamed, all reach the client |
| `*_shutdown_drains` | `SIGTERM` while a response streams through the proxy: it ends whole, the server exits 0 with its report, and the proxy then answers `502` |

Differences between the proxies, none of them a server fault:

- nginx appends to a client's `X-Forwarded-For` (`203.0.113.9, 127.0.0.2`);
  Caddy, trusting no proxies by default, replaces it (`127.0.0.2`). With
  only the proxy trusted, `client_of` gives `127.0.0.2` for both.
- nginx sends `100 Continue` itself and buffers the body. Caddy streams it
  and also relays the server's own `100 Continue`, so the client sees two
  interim responses, which RFC 9110 section 15.2 requires clients to accept.
- Caddy offers HTTP/2 to TLS clients and speaks HTTP/1.1 to the server.
- After relaying an answer sent before a large body was read, Caddy closes
  the client's connection with the rest of the body unread, so a client
  still sending may see a reset after the answer. The test accepts that
  reset (curl exit 55) for Caddy only.
- nginx reuses connections to the server only with an `upstream` block
  with `keepalive`, `proxy_http_version 1.1` and an empty `Connection`
  header, as in the test configuration.

The tests found one server defect (issue #142). A refusal, or an answer
sent before the body was read, closed lingering after discarding at most
64 KiB. A proxy forwards the whole body regardless, so it was often still
sending when the server closed; the connection reset under it, and it
answered `502` instead of passing on the server's answer. The budget is now 16 MiB, still
within 2 s. Twenty requests per case, measured on 2026-10-17:

| Request | Proxy | 64 KiB budget | 16 MiB budget |
|---|---|---|---|
| 2 MiB sized body, `413` | nginx, buffering | 14 of 20 `502` | 0 |
| 2 MiB chunked body, `413` | nginx, buffering | 15 of 20 `502` | 0 |
| 2 MiB sized body, `413` | nginx, streaming | 2 of 20 `502` | 0 |
| 1000 KiB sized body, early `200` | nginx, buffering | 1 of 20 `502` | 0 |
| 1000 KiB chunked body, early `200` | nginx, buffering | 7 of 20 `502` | 0 |
| 2 MiB bodies, `413`, four variants | Caddy | 1 to 3 of 20 `502` each | 0 |
| 1000 KiB chunked body, early `200` | Caddy | 1 of 20 `502` | 0 |

With 200 KiB bodies neither proxy saw a `502` under either budget.

## Deployment

For a public service, put a reverse proxy in front of the server (see
[deploying behind a reverse proxy](http-proxy.md#deploying-behind-a-reverse-proxy)).
The proxy also absorbs slow clients beyond what `idle_connections` bounds:

- Buffer whole request heads, and preferably bodies, before forwarding
  (nginx does both by default; keep `proxy_request_buffering on`).
  `tests/fixtures/proxy/` has tested nginx and Caddy configurations.
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
cargo test --locked --all-features --test integration test_std_http_server_qualification::
PLENTY_DOCKER_TESTS=1 cargo test --locked --test integration test_std_http_server_proxy::
cargo build --release --locked
python3 examples/http_server_bench.py --compiler target/release/plenty --output target/http-bench --runs 3
python3 examples/http_server_bench.py --compiler target/release/plenty --output target/https-bench --runs 3 --tls
python3 fuzz/http_server/run.py --time 1800 --jobs 2
```

The curl tests are skipped when curl is not installed. The proxy tests
need Docker and curl and are skipped, with a message, unless
`PLENTY_DOCKER_TESTS=1` (see [reverse-proxy tests](../../development/workflow.md#reverse-proxy-tests));
the `proxy_tests` job in `.github/workflows/ci.yml` runs them on every push
and pull request. The benchmark needs ApacheBench (`ab`) and `taskset`, and
with `--tls` also curl. Fuzzing needs clang's libFuzzer; without it the
libFuzzer tests in `test_fuzz.rs` print a note and pass, unless
`PLENTY_FUZZ_TESTS=1`. Busy machines distort the benchmark: check
the load average first, and keep the total number of busy threads at or
below the number of physical cores.
