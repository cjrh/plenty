# Completed library and application work

[Completed-work index](../completed-work.md) · [Remaining work](../backlog.md)

The initial library slices below shipped. “Complete” refers to that bounded
slice, not every extension of a module. The backlog owns all remaining
candidates; linked references define current behavior and platform limits.

## B38: Library breadth

Issue #49's initial B39–B70 slices are delivered. Modules are implemented in
Plenty unless native complexity/security warrants a wrapper (issue #505,
local PR #578); large native dependencies use separately packaged
[engines](../design/toolchain/01-building/engines.md) (issue #504, local PR #579).
The [catalogue rationale](../proposals/standard-library-scope.md) records the
scope decision, not a second task list.

| Delivery | Contract / evidence |
| --- | --- |
| [XML](../design/stdlib/xml.md) | Bounded UTF-8 events and validating writes without DTDs/external entities (issue #462). |
| [Filesystem watch](../design/stdlib/fs/watch.md) | Bounded Linux inotify batches, overflow reporting and readiness integration (issue #484). |
| [Templates](../design/stdlib/template.md) | Bounded Jinja-style interpolation, filters, conditions, named inheritance and HTML escaping over explicit JSON (issue #499). |
| [Linalg](../design/stdlib/linalg.md) | Finite f64 matrices, solves and dense decompositions over allocation-safe faer kernels, with engine-parameterized build/packaging/CI (issue #490). |
| [MIME](../design/stdlib/mime.md) | RFC 9110 media parsing/building/formatting/matching and the extension table used by HTTP files (issue #485). |
| [Images](../design/stdlib/image.md) | Plenty over `std.compress`: bounded owned 8-bit buffers, all PNG colour types/depths/Adam7 decoding, encoding, crop, conversion and nearest/bilinear resizing; dimensions/bytes checked before allocation (issue #476). |
| [Statistics](../design/stdlib/stat.md) | Finite-f64 descriptions, inclusive quantiles and normal/uniform/exponential distributions with explicit fixed-draw sampling (issue #494). |
| [Sketches](../design/stdlib/sketch.md) | Dense HyperLogLog, reservoir quantiles, Misra–Gries bounds, static xor/scalable cuckoo filters, bounded memory and versioned checkpoints (issues #496–#498). |
| [Key/value database](../design/stdlib/kv.md) | Named ordered byte maps, transactions and bounded scans in Plenty over SQLite; versioned file schema and [engine decision](../design/stdlib/kv-engine.md) (issue #495). |
| [UUID](../design/stdlib/uuid.md) | Canonical text/network bytes, OS-backed v4/v7 and allocation-free numeric sort keys (issue #461). |
| [System](../design/stdlib/system.md) | Bounded Linux procfs snapshots for CPU/memory/load/uptime/block devices/disk I/O/processes (issue #487). |

Remaining breadth is **B38** in the [backlog](../backlog.md).

## B39–B70: Initial delivery slices

The local issues listed here recorded acceptance checks, ownership/allocation
contracts and prerequisites. They are completion evidence, not active task
assignments. Extensions retain their original B IDs in the
[standard-library follow-ups](../backlog.md#standard-library-follow-ups).

| ID / local issue | Delivered subset |
| --- | --- |
| B39 / #76 | [Bytes](../design/stdlib/bytes.md): owned buffers, checked ranges, explicit copies, borrowed access, list-style indexing, loops, native fill and bulk scans (issue #115). Copying slices (issue #579), borrowed `&[u8]` subranges and mutable borrowed loops (issues #581/#594) shipped. Static `bytes` literals and `ByteSource` support in byte operations and std stream writes shipped (issue #156, local PRs #198/#203). Original lossless Unix transport remains `list[u8]` (issue #61). |
| B40 / #77 | [Binary streams](../design/stdlib/io.md): statically dispatched reader/writer/seek/close, explicit partial progress, exact/bounded reads/writes/copy, fixed buffering and distinct flush/close failures, with file/process/memory adapters over Bytes. |
| B41 / #78 | [Error context](../design/stdlib/error.md): typed operation/path/cause, explicit conversion, descriptions, stderr fallback reports, unit Results (issue #110), `Formattable` causes and readable entrypoint reports. |
| B42 / #79 | [Time](../design/stdlib/time.md): checked durations, monotonic instants/deadlines, timestamps and sleep. Calendar/time-zone/RFC 3339 support is the official external `plentylang/tz` package (issue #463); `Zoned.until`/`since` calendar differences shipped in v0.3.0 (issue #566). |
| B43 / #80 | [Environment](../design/stdlib/env.md)/[process](../design/stdlib/process.md): lossless variables/snapshots/cwd, argument-vector spawning without a shell, child env/cwd, piped streams, bounded deadlock-free collection, exit/signal status, termination and reaping. [Shlex](../design/stdlib/shlex.md) splits/quotes shell words without executing/expanding (issue #478). |
| B44 / #81 | [CLI](../design/stdlib/cli.md): validated schemas, typed/repeatable arguments, `--`, help/errors and lossless paths. [Progress](../design/stdlib/cli-progress.md) adds terminal-only throttled bars/spinners/counts/rates/ETA (issue #477); [fileinput](../design/stdlib/cli-fileinput.md) adds buffered fallible line iteration over native/raw command-line paths and stdin fallback (issue #553). |
| B45 / #82 | [Formatting](../design/stdlib/fmt.md): static display/debug contracts, scalar/callback aggregate formatting, bounded partial-progress streaming and fallible strings. F-strings add field expressions, conversions, debug `=`, checked Python-style specs and nested fill/alignment/width/precision, returning `FormatError`; formatter output avoids intermediate strings (issue #157, local PRs #200/#202). |
| B46 / #83 | [Logs](../design/stdlib/log.md): leveled typed events and generic caller-selected sinks (logfmt text, bounded memory and nonblocking channel/drain), clock policies and bounded lines. |
| B47 / #84 | [Text](../design/stdlib/text.md) builders/join/scalar wrapping and [Unicode 17 normalization](../design/stdlib/unicode.md), combining-class/Mark classification. UAX #29 segmentation/UAX #11 width ship in external `plentylang/unicode-text` (issue #465). |
| B48 / #85 | [Regex](../design/stdlib/regex.md): Plenty Pike VM with explicit pattern/nesting/repetition/instruction/memory/work/output/match limits; text/bytes captures, iteration, split and replacement. |
| B49 / #86 | [Collections](../design/stdlib/collections.md): natural/comparator heaps and generator drains, including tuple ordering/heap values (issue #111). B75 pipelines superseded the former `std.iter` adapters. |
| B50 / #87 | [Random](../design/stdlib/random.md): owned version-stable seeded xoshiro256** generators, unbiased ranges, shuffle/index sampling/byte fill. [Secrets](../design/stdlib/secrets.md): OS-only random fill/tokens/unbiased integers over `getrandom`, with no fallback. |
| B51 / #88 | [JSON](../design/stdlib/json.md): strict RFC 8259 event decoder/validating encoder/value tree, depth/input/output/duplicate-key limits, located errors and std.io integration. |
| B52 / #89 | [TOML](../design/stdlib/toml.md): bounded strict TOML 1.0 parser/writer with located errors. [Format-preserving edits](../design/stdlib/toml-editing.md) add atomic in-memory changes and explicit path limits (issue #475). |
| B53 / #90 | [CSV](../design/stdlib/csv.md): streaming reusable-record reader/writer, explicit delimiter/quote/newline/width/size policies, strict RFC 4180 quoting and positioned errors. |
| B54 / #91/#489 | [Encoding](../design/stdlib/encoding.md): strict hex/base64/base64url/base32/base32hex, MIME wrapping, validated alphabets/padding and explicit-endian integer packing. |
| B55 / #92 | [Filesystem](../design/stdlib/fs.md): descriptor-rooted no-follow operations, anonymous publication, bounded copy/delete and byte globs; [tempfile](../design/stdlib/tempfile.md) resources with explicit retention/cleanup. [Directories](../design/stdlib/fs/dirs.md) resolves HOME/XDG config/cache/data/state (issue #472), with Linux trusted-parent/publication limits documented. |
| B56 / #93 | Explicit thread/sync/channel/concurrent modules preserve builtin identities and ownership/effect guarantees. [Atomics](../design/stdlib/atomic.md) and [mutexes](../design/stdlib/sync.md) shipped (local PR #362). |
| B57 / #94 | [Test support](../design/stdlib/test.md): located assertions, Scratch, seeded RNG, fake clock/stream failures and short transfers; parallel package test processes, filters/list/capture/jobs/timeouts/seeds, per-test TMPDIR and allocation/file-I/O failure injection (local PR #142). |
| B58 / #95 | Every native primitive declares a worker effect, without std-wide exemption. [Readiness](../design/stdlib/ready.md) supports bounded waits/cancellation and many-handle selection with direct token wakeups. |
| B59 / #96 | [Sockets/DNS](../design/stdlib/net.md): owned TCP/UDP, bounded deadline/token resolver, descriptor-limit errors and measured accept bounds (local PRs #135/#140); immediate `accept_now`/`read_now`/`write_now` report WouldBlock for selector-driven TLS (issue #527). Initial platform: x86-64 Linux; Unix-domain sockets were declined. |
| B60 / #97 | [URL](../design/stdlib/url.md): strict RFC 3986 absolute parsing/validated formatting, component-aware percent encoding, query pairs and bulk scans (issue #115). |
| B61 / #98 | [TLS](../design/stdlib/tls.md): verified TLS 1.2/1.3 client/server streams over std.net using vendored rustls/ring; PEM/system roots, deadlines/cancellation, certificate errors, client certs, ALPN/SNI, version/cipher choice, CRLs and selector-driven try operations (local PRs #145/#147). Recoverable rustls allocation, resumption/0-RTT and OCSP were declined for this slice. |
| B62 / #99 | [HTTP/1.1](../design/stdlib/http.md): bounded head read/write and content-length/chunked/close-delimited framing, strict ambiguous-frame rejection and mutation corpus (local PR #139). |
| B63 / #100 | [HTTP client](../design/stdlib/http-client.md): pooling, bounded deadlines/redirects, cross-origin credential stripping, proxies and idempotent replay; verified HTTPS, CONNECT, streamed uploads/Expect (local PRs #146/#154). Shared-client locking/atomics (issue #326, local PR #369) and pool accounting/cancellation fixes (issue #333, local PR #378) landed. `ws`/`wss` clients shipped (issue #467, local PR #593). Cookie jar/content decoding were declined; callers retain their own policies. |
| B64 / #101 | [HTTP server](../design/stdlib/http-server.md): ordered typed routing, bounded admission/fixed workers, deadlines/logging, transport abstraction, graceful shutdown, parked idle keep-alives and accept-timed HTTPS handshakes (local PRs #143/#148). WebSocket upgrades/shutdown (issue #466, local PR #593) and detached close-delimited [SSE](../design/stdlib/sse.md) shipped (issue #468, local PR #597). |
| B65 / #102 | Bounded forms/JSON/multipart temporary uploads, validated signed cookies/rotation, reverse-proxy/direct-TLS guidance and root-confined conditional static files (local PRs #144/#152). See [body](../design/stdlib/http-body.md), [cookie](../design/stdlib/http-cookie.md), [proxy](../design/stdlib/http-proxy.md) and [files](../design/stdlib/http-files.md). |
| B66 / #103 | [HTTP production qualification](../design/stdlib/http-qualification.md): framing mutations, curl/TLS interop, slow clients/load/shutdown, allocation/descriptor/heap failures, relocated tools and backend linkage (local PR #157). Host/absolute-form validation, parked heads/handshakes, signal drain, lingering close and TLS Finished acknowledgement fixes landed (issues #131–#133/#138/#142/#148). Nginx/Caddy reverse-proxy Docker checks, installed CLI/data and JSON-service workflows, HTTPS baselines and libFuzzer integration shipped (local PRs #171/#180/#183/#186). Production-ready label: 2026-10-18. Typed application routes shipped (issue #159, local PR #199). |
| B67 / #104 | [Crypto](../design/stdlib/crypto.md): SHA-1 protocol use, SHA-256/512, HMAC and constant-time comparison via RustCrypto; BLAKE3 incremental/keyed/derivation (issues #473/#483). AEAD, Ed25519, ephemeral X25519 and HKDF use ring (issue #464). RSA/P-256, SPKI/PKCS#8/JWK import, strict PEM and Wycheproof checks shipped (issue #561); [JWT](../design/stdlib/jwt.md) builds on them (issue #488, local PR #697). RSA arenas make allocation failure explicit (issue #567); Ed25519 small-order rejection uses `SmallOrderKey`, while mixed-order keys remain accepted (issue #569). |
| B68 / #105 | [Compression](../design/stdlib/compress.md): bounded gzip/zlib/DEFLATE buffers/streams over zlib-rs, member/trailing-data/checksum policies and fixed per-stream memory. [Brotli](../design/stdlib/compress-brotli.md) qualities 0–11 support bounded streams and fallible native allocation (issue #491). |
| B69 / #106 | [Archives](../design/stdlib/archive.md): TAR ustar/pax/GNU and ZIP/ZIP64 stored/DEFLATE streaming with root-confined staging extraction under explicit policy. |
| B70 / #107 | [SQLite](../design/stdlib/sqlite.md): vendored 3.54.0, typed statements, owned rollback-on-drop transactions, copied values and explicit length limits. |

## B71: Error composition and HTTP

The language/reporting foundation and precise-error migration are delivered:
general sums, enum methods, structural protocols, `Formattable`, typed context,
entrypoint reports and typed HTTP handlers. `std.error.map_error` and
`Result.map_error` accept payload variants as function values (issue #556).
Explicit retained-job errors/`drop_job()` preserve start/submission causes;
`Failure` remains a last resort.

[Typed I/O](../design/stdlib/io-typed.md) supplies stream helpers/legacy bridges.
[Typed HTTP](../design/stdlib/http-typed.md) body readers preserve concrete causes
and partial progress, validate parsed framing and guard connection recovery.
Typed parsing/writes/buffering serve the whole HTTP client, including CONNECT,
uploads/redirects/pooling; stored-error retrieval was removed while the legacy
Reader bridge and typed response composition remain.

Installed CLI/data and JSON-service workflows validate causes, reports, exit
status, recovery and cleanup (local PR #472). See the
[composition evidence](../proposals/completed/error-composition/streams-and-evidence.md).
Remaining presentation/inference candidates stay **B71** in the [backlog](../backlog.md).

## B76: Columnar data

The optional [dataframe](../design/stdlib/dataframe.md) foundation includes bulk
numeric/string/bool construction/extraction, nullable schema inspection,
conditional/cast/string/distinct/window expression plans, grouping, five join
kinds, stable sorting/windows and CSV/Parquet streaming (issues #448–#451).
The Polars engine is reached-only and separately packaged; version-matched
release archives, compatibility/notices and deployment checks shipped (issue #456).
Integer-key checked as-of joins shipped (issue #453); Date/Datetime units,
naive/UTC metadata, nullable epochs and calendar formatting followed (issue #457).
Numeric widths, owned binary columns and nested list/struct schema/access/
Parquet interchange shipped with issues #458–#460, preserving parent/child nulls.
The [original spike](../proposals/polars-dataframes/evidence.md) records its evidence.

The [sales report application](../tutorial/special-purpose/dataframe-application.md)
and `examples/sales_report/` exercise catalogue validation, CSV/partitioned
Parquet, nullable cleanup, UTC daily grouping and audited exports, with independent
checks and an optional scaled workload. Classified rows are materialized once.

Primitive-field [clist](../design/facilities/01-builtins/clists.md) provides
native-width packed columns, allocation-free empty owners, transactional
reserve/append, scalar indexed fields, explicit row copies and borrowed
scalar-valued pipelines. Ordered removal/clear retain capacity without allocating.
[Direct indexed access](../proposals/columnar-list/indexed-access.md) avoids
per-field runtime dispatch (issue #432); bounded optimized integer count/filter
SIMD is recorded under [B23](performance.md#b23-simd). It remains distinct from
Polars-backed dataframes. [Columnar evidence](../proposals/columnar-list.md)
records whole-row tradeoffs. Remaining scope stays **B76** in the [backlog](../backlog.md).

## B78: Diffs

[Myers/patience diffs](../design/stdlib/diff.md) over lists, Bytes and
line/word/char tokens produce GNU-compatible unified output (issue #469,
local PR #589). `max_cost` optionally bounds Myers work (issue #510,
local PR #596). The scalar-list codegen improvement is recorded under
[B19](performance.md#b19-generated-code-and-storage), not an unfinished diff fix.
Extensions stay **B78** in the [backlog](../backlog.md).

## B79: Markdown

[Markdown](../design/stdlib/markdown.md) is a Plenty port of commonmark.js,
targeting CommonMark 0.31.2 with events/HTML and input/depth/output limits
(issue #470, local PR #595). Extensions stay **B79** in the [backlog](../backlog.md).

## B80: Plotting

The partial [plotting implementation](../design/stdlib/plot.md) delivers owned
tables/record accessors, layer-specific data/inherited mappings, nine geoms
(including supplied errors/ranges/ribbons), grouped counts/binning/bar positions,
wrapped/grid facets, transformed continuous/discrete position scales with breaks/
labels, palettes/gradients, basic themes/legends, wrapped text layout preflight
and streaming SVG. This is not a claim that the accepted core milestone is
finished; its remaining scope stays **B80** in the [backlog](../backlog.md).

## B81: LRU cache

[LRU](../design/stdlib/cache-lru.md) implements fixed-capacity expected-O(1)
operations in Plenty with no allocation after construction (issue #482,
local PR #613). Copying getters followed use-site generic checking
(issues #515/#516, local PR #619); optional reference/mutable getters replaced
callback/trapping getters (local PR #623). No separate outstanding B81 item was
recorded; future additions need their own use case/scope.

## B82: Graphs

[Graphs](../design/stdlib/graph.md) deliver stable directed/undirected handles,
BFS/DFS, topological sort, SCC/components, Dijkstra, A*, Bellman–Ford and Kruskal
(issue #486, local PR #614). Extensions stay **B82** in the [backlog](../backlog.md).

## B85: Actors

[Actors](../design/stdlib/actor.md) deliver supervised single-thread execution,
per-actor heaps on x86-64 Linux, bounded copied-message mailboxes and trap
containment (issue #536, local PR #657). Typed trap exits, heap-safe spawn
checks, allocation-free timers, timeout/gone request-reply and opt-in one-for-one
fixed-window restart shipped (issue #537, local PR #658).

[Actor networking](../design/stdlib/actor-net.md) adds actor-per-connection
listeners, pause/refuse admission, bounded back-pressure buffers, one-timer
idle/stall/read deadlines and nonblocking dial/lookup (issue #538, local PR #660).
[Actor TLS](../design/stdlib/actor-tls.md) uses a nonblocking TLS session
(issue #543, local PR #664). General Watchable handles, named readiness,
spawn tags on Down and in-place connection-buffer compaction shipped
(issues #540–#542/#545, local PRs #661/#662).
Remaining stages stay **B85** in the [backlog](../backlog.md).
