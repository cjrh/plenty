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
| [Advisory file locks](../design/stdlib/fs/lock.md) | Owned shared/exclusive Linux locks, nonblocking acquisition and cancellable deadline waits; close-only lifetime preserves inherited descriptions (issue #774), with [explicit child inheritance](#explicit-child-file-inheritance) (issue #777). |
| [Filesystem watch](../design/stdlib/fs/watch.md) | Bounded Linux inotify batches, overflow reporting and readiness integration (issue #484). |
| [Templates](../design/stdlib/template.md) | Bounded Jinja-style interpolation, filters, conditions, named inheritance and HTML escaping over explicit JSON (issue #499); reusable bounded text and quote-specific attribute escaping (issue #823). |
| [Linalg](../design/stdlib/linalg.md) | Finite f64 matrices, solves and dense decompositions over allocation-safe faer kernels, with engine-parameterized build/packaging/CI (issue #490). |
| [MIME](../design/stdlib/mime.md) | RFC 9110 media parsing/building/formatting/matching and the extension table used by HTTP files (issue #485). |
| [Images](../design/stdlib/image.md) | Plenty over `std.compress`: bounded owned 8-bit buffers, all PNG colour types/depths/Adam7 decoding, encoding, crop, conversion and nearest/bilinear resizing; dimensions/bytes checked before allocation (issue #476). |
| [Statistics](../design/stdlib/stat.md) | Finite-f64 descriptions, inclusive quantiles and normal/uniform/exponential distributions with explicit fixed-draw sampling (issue #494). |
| [Sketches](../design/stdlib/sketch.md) | Dense HyperLogLog, reservoir quantiles, Misra–Gries bounds, static xor/scalable cuckoo filters, bounded memory and versioned checkpoints (issues #496–#498). |
| [Key/value database](../design/stdlib/kv.md) | Named ordered byte maps, transactions and bounded scans in Plenty over SQLite; versioned file schema and [engine decision](../design/stdlib/kv-engine.md) (issue #495). |
| [UUID](../design/stdlib/uuid.md) | Canonical text/network bytes, OS-backed v4/v7 and allocation-free numeric sort keys (issue #461). |
| [System](../design/stdlib/system.md) | Bounded Linux procfs snapshots for CPU/memory/load/uptime/block devices/disk I/O/processes (issue #487). |

Remaining breadth is **B38** in the [backlog](../backlog.md).

Issue #705 corrects the [archive PAX decimal parser](../design/stdlib/archive.md#limits)
to accept every nonnegative `i64`, including its final eight values. Boundary
tests cover both optimization modes, overflow and malformed digits; the separate
TAR content-length cap remains 2⁶² bytes.

## B39–B70: Initial delivery slices

The local issues listed here recorded acceptance checks, ownership/allocation
contracts and prerequisites. They are completion evidence, not active task
assignments. Extensions retain their original B IDs in the
[standard-library follow-ups](../backlog.md#standard-library-follow-ups).

| ID / local issue | Delivered subset |
| --- | --- |
| B39 / #76 | [Bytes](../design/stdlib/bytes.md): owned buffers, checked ranges, explicit copies, borrowed access, list-style indexing, loops, native fill and bulk scans (issue #115). Copying slices (issue #579), borrowed `&[u8]` subranges and mutable borrowed loops (issues #581/#594) shipped. Static `bytes` literals and `ByteSource` support in byte operations and std stream writes shipped (issue #156, local PRs #198/#203). Original lossless Unix transport remains `list[u8]` (issue #61). |
| B40 / #77 | [Binary streams](../design/stdlib/io.md): statically dispatched reader/writer/seek/close, explicit partial progress, exact/bounded reads/writes/copy, fixed buffering and distinct flush/close failures, with file/process/memory adapters over Bytes. [Sequential binary Cursor fields](../design/stdlib/io-binary.md) add checked reads/writes, payload caps and contextual truncation (issue #658). |
| B41 / #78 | [Error context](../design/stdlib/error.md): typed operation/path/cause, explicit conversion, descriptions, stderr fallback reports, unit Results (issue #110), `Formattable` causes and readable entrypoint reports. |
| B42 / #79 | [Time](../design/stdlib/time.md): checked durations, monotonic instants/deadlines, timestamps and sleep; [typed std time APIs and cancellable waits](../completed-work.md#typed-time-and-cancellable-waits) (issue #635). Calendar/time-zone/RFC 3339 support is the official external `plentylang/tz` package (issue #463); `Zoned.until`/`since` calendar differences shipped in v0.3.0 (issue #566). |
| B43 / #80 | [Environment](../design/stdlib/env.md)/[process](../design/stdlib/process.md): lossless variables/snapshots/cwd, argument-vector spawning without a shell, child env/cwd, piped streams, bounded deadlock-free collection, exit/signal status, termination and reaping. [Explicit child-file inheritance](#explicit-child-file-inheritance) selects owned files and lock copies (issue #777). [Shlex](../design/stdlib/shlex.md) splits/quotes shell words without executing/expanding (issue #478). |
| B44 / #81 | [CLI](../design/stdlib/cli.md): validated schemas, typed/repeatable arguments, `--`, help/errors and lossless paths. [Progress](../design/stdlib/cli-progress.md) adds terminal-only throttled bars/spinners/counts/rates/ETA (issue #477); [fileinput](../design/stdlib/cli-fileinput.md) adds buffered fallible line iteration over native/raw command-line paths and stdin fallback (issue #553). |
| B45 / #82 | [Formatting](../design/stdlib/fmt.md): static display/debug contracts, scalar/callback aggregate formatting, bounded partial-progress streaming and fallible strings. F-strings add field expressions, conversions, debug `=`, checked Python-style specs and nested fill/alignment/width/precision, returning `FormatError`; formatter output avoids intermediate strings (issue #157, local PRs #200/#202). |
| B46 / #83 | [Logs](../design/stdlib/log.md): leveled typed events and generic caller-selected sinks (logfmt text, bounded memory and nonblocking channel/drain), clock policies and bounded lines. |
| B47 / #84 | [Text](../design/stdlib/text.md) builders/join/scalar wrapping, [Python-style paragraph tools](../design/stdlib/text-wrapping.md) (issue #654), and [Unicode 17 normalization](../design/stdlib/unicode.md), combining-class/Mark classification. UAX #29 segmentation/UAX #11 width ship in external `plentylang/unicode-text` (issue #465). |
| B48 / #85 | [Regex](../design/stdlib/regex.md): Plenty Pike VM with explicit pattern/nesting/repetition/instruction/memory/work/output/match limits; text/bytes captures, iteration, split and replacement. |
| B49 / #86 | [Collections](../design/stdlib/collections.md): natural/comparator heaps and generator drains, including tuple ordering/heap values (issue #111). B75 pipelines superseded the former `std.iter` adapters. |
| B50 / #87 | [Random](../design/stdlib/random.md): owned version-stable seeded xoshiro256** generators, unbiased ranges, shuffle/index sampling/byte fill. [Secrets](../design/stdlib/secrets.md): OS-only random fill/tokens/unbiased integers over `getrandom`, with no fallback. |
| B51 / #88 | [JSON](../design/stdlib/json.md): strict RFC 8259 event decoder/validating encoder/value tree, depth/input/output/duplicate-key limits, located errors and std.io integration. |
| B52 / #89 | [TOML](../design/stdlib/toml.md): bounded strict TOML 1.0 parser/writer with located errors. [Format-preserving edits](../design/stdlib/toml-editing.md) add atomic in-memory changes and explicit path limits (issue #475). |
| B53 / #90 | [CSV](../design/stdlib/csv.md): streaming reusable-record reader/writer, explicit delimiter/quote/newline/width/size policies, strict RFC 4180 quoting and positioned errors. |
| B54 / #91/#489/#656 | [Encoding](../design/stdlib/encoding.md): strict hex/base64/base64url/base32/base32hex, MIME wrapping, validated alphabets/padding and explicit-endian integer/bit-exact IEEE binary32/binary64 packing. |
| B55 / #92 | [Filesystem](../design/stdlib/fs.md): descriptor-rooted no-follow operations, anonymous publication, bounded copy/delete and byte globs; [tempfile](../design/stdlib/tempfile.md) resources with explicit retention/cleanup. [Recursive copying and selected metadata preservation](../design/stdlib/fs/copying.md) add shared bounds, explicit partial-tree recovery and strict ordinary permissions/timestamps (issues #659/#660). [Directories](../design/stdlib/fs/dirs.md) resolves HOME/XDG config/cache/data/state (issue #472), with Linux trusted-parent/publication limits documented. Explicit [private creation modes and held-handle permission updates](../design/stdlib/fs/helpers.md#private-creation-and-permission-changes) avoid create-then-chmod windows (issue #773). [Link creation and canonicalization](../design/stdlib/fs/metadata.md#creating-and-resolving-links) preserve native bytes and OS errors on workers, with explicit path limits and no confinement guarantee (issue #775). |
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
| B66 / #103 | [HTTP production qualification](../design/stdlib/http-qualification.md): framing mutations, curl/TLS interop, slow clients/load/shutdown, allocation/descriptor/heap failures, relocated tools and backend linkage (local PR #157). Host/absolute-form validation, parked heads/handshakes, signal drain, lingering close and TLS Finished acknowledgement fixes landed (issues #131–#133/#138/#142/#148). Nginx/Caddy reverse-proxy Docker checks, installed CLI/data and JSON-service workflows, HTTPS baselines and libFuzzer integration shipped (local PRs #171/#180/#183/#186). Production-ready label: 2026-10-18. Typed application routes shipped (issue #159, local PR #199). Proxy fixture port leases prevent cooperating same-process HTTP/TLS reuse across socket handoff and through cleanup (issue #719); deterministic non-Docker ownership tests cover concurrency and unwind. Separate parallel native qualification passes all ten worker/actor nginx/Caddy cases with authenticated removals and unchanged foreign containers (local PR #918), not a whole-workspace gate. External bind races remain; see [reverse proxies](../design/stdlib/http-qualification.md#reverse-proxies). |
| B67 / #104 | [Crypto](../design/stdlib/crypto.md): SHA-1 protocol use, SHA-256/512, HMAC and constant-time comparison via RustCrypto; BLAKE3 incremental/keyed/derivation (issues #473/#483). AEAD, Ed25519, ephemeral X25519 and HKDF use ring (issue #464). RSA/P-256, SPKI/PKCS#8/JWK import, strict PEM and Wycheproof checks shipped (issue #561); [JWT](../design/stdlib/jwt.md) builds on them (issue #488, local PR #697). RSA arenas make allocation failure explicit (issue #567); Ed25519 small-order rejection uses `SmallOrderKey`, while mixed-order keys remain accepted (issue #569). |
| B68 / #105 | [Compression](../design/stdlib/compress.md): bounded gzip/zlib/DEFLATE buffers/streams over zlib-rs, member/trailing-data/checksum policies and fixed per-stream memory. [Brotli](../design/stdlib/compress-brotli.md) qualities 0–11 support bounded streams and fallible native allocation (issue #491). |
| B69 / #106 | [Archives](../design/stdlib/archive.md): TAR ustar/pax/GNU and ZIP/ZIP64 stored/DEFLATE streaming with root-confined staging extraction under explicit policy. |
| B70 / #107 | [SQLite](../design/stdlib/sqlite.md): vendored 3.54.0, typed statements, owned rollback-on-drop transactions, copied values and explicit length limits. |

## B48: Regex acceleration

Issue #742 adds bounded ASCII prefix filters across alternatives and classes,
a lazy DFA for existence and fixed-length match offsets, and a one-pass engine
for unambiguous ASCII transitions. The Pike VM preserves subgroup captures and
handles unsupported patterns, Unicode fallback and exhausted DFA caches. Work
limits include accelerated scans and fallbacks; optional scratch fits within
the existing memory budget, and allocations remain fallible. Issue #752 adds
pattern and search-budget compatibility checks before a cursor reuses cached
transitions. Issue #753 removes per-byte text calls from the ASCII engines
by borrowing UTF-8 storage once per search, iterates byte slices directly in
the prefix scanner, and applies that filter at DFA restart states. The full
regex-redux workload uses 44.36% fewer instructions than the merged #742
implementation. Issue #758 adds separate bit lanes for small alternative sets
and bounded native substring batches for at most eight short ASCII prefixes.
The unchanged full workload uses another 11.39% fewer instructions than main
`d951457d`, with identical reference output. Scalar fallback lowering is
isolated with CLIF variants and tracked in issue #760.

The unchanged regex-redux program measures cleaning, all nine variant counts,
and five substitutions against the Rust `regex` reference. Reproducible
instruction-count results and the C/Plenty prefix-loop comparison are recorded
in `experiments/regex/README.md`. See the [regex costs and coverage](../design/stdlib/regex.md#costs)
and [benchmark workflow](../development/benchmarks-game.md). B48 retains broader
engine coverage, scalar scan lowering and Unicode feature expansion.

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

Issue #634 unifies report and display formatting on
`std.fmt.Formatter[W]` and `FormatError`. Report methods use a method-local
writer parameter, including anonymous-sum dispatch and conditional typed
contexts. Reports can use arbitrary writers and floating-point format specs;
stderr text/integer/boolean reports still allocate no memory. Canonical owned
text methods on URLs, media types, socket addresses and Set-Cookie values are
`to_string()`. See [report formatting](../design/stdlib/report.md).

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
per-actor heaps on x86-64 and AArch64 GNU/Linux, bounded copied-message mailboxes and trap
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

[`std.actor.http.server`](../design/stdlib/actor-http-server.md) adds a second
HTTP/1.1 server with isolated connection construction, bounded complete-request
handlers, resumable TCP/TLS streaming, deadlines and draining. The shared
[`std.http.decoder`](../design/stdlib/http-decoder.md) supplies byte-feed
request framing. Stage observations, isolated spawning and immediate/destructor
shutdown support its lifecycle; runnable deadline and live-stage cleanup fixes
address issues #638 and #639. Fair child-exit delivery and safe runnable-slot
reuse under connection churn address issues #640 and #642. Scalar-wrapper list
references use their actual storage width (issue #643), preventing neighboring
actor-address entries from being overwritten. The [qualification record](../design/stdlib/actor-http-qualification.md)
separates implementation delivery from production readiness and comparative
measurements. The worker HTTP server remains independently supported.
The [qualification record](../design/stdlib/actor-http-qualification.md) includes
540,000 successful matched HTTP/TLS benchmark requests, a mixed-failure run
with 400 contained traps and stable resources, and relocated native binaries.
TLS sessions reconstruct a private configuration graph within their actor heap,
so trapped sessions and repeated configuration rotation release retired generations
without dropping interrupted rustls state (issue #641). This adds per-session
configuration memory and setup work; the original TLS comparison predates that
change. Completion acknowledgements and bounded retry timers reduce concurrent
keep-alive mailbox retry overhead while preserving application mail, completion
accounting and shutdown control (issue #644). The qualification record retains
the original comparison and adds a controlled before/after HTTP measurement. A
[refreshed TLS comparison](../design/stdlib/actor-http-tls-comparison.md)
after both fixes records 270,000 successful requests, 9,000 verified handshakes,
current idle RSS and 400 contained traps with stable after-batch resources.

A test-only [budget and retained-stack investigation](../proposals/actor-budget-scheduling.md)
exercises the production heap router and trap boundary on two guarded stacks.
Explicit checkpoints demonstrate peer progress, retained nested local borrows,
trap recovery after resumption and cooperative cancellation with cleanup. It
adds no production scheduling behavior; compiler insertion and the stage's
shared-context ownership remain unimplemented. See the
[probe evidence and limits](../proposals/actor-budget-scheduling/evidence.md).

Remaining stages stay **B85** in the [backlog](../backlog.md).

## Standard-library argument and error contracts

B92's allocation/error-type work (local PR #804) gives std domain errors an
`Allocation(AllocError)` cause, narrows allocation-only byte operations to
`AllocError`, and reports invalid caller positions with named traps. Stream
reads and `read_exact` validate buffer ranges before transport access, EOF,
handshake work, or stored errors; TLS session transfers follow the same rule
(issue #666). Encoding fields, UUID output, crypto finalization and CSV field
positions trap on invalid caller offsets or indices; operations with no other
failure return plain values (issue #667).

Unused byte-error adapters and unreachable range variants have been removed
(issue #668). Real failures remain recoverable: truncation, malformed encoded
text, allocation failure, transport failures and the capacities, limits and
seek targets that still return named errors. `BytesError.OutOfRange`, kept
then for actor networking connection limits, was removed by
[B95](language-and-tooling.md#b95-std-limit-domains).
See the [invalid-argument convention](../design/stdlib/conventions.md#invalid-arguments)
and [byte-stream contract](../design/stdlib/io.md).

B92's remaining worker-certification subset is delivered in issue #661. Thin
function and variant-constructor values are certified against all matching
function addresses in the closed program. The 30 pure allocation adapters
introduced by local PR #804 now use direct `map_error` calls at 227 current
call sites, including nested domain-error wrapping. Helpers that attach
partial-progress context retain that behavior. See
[worker callable certification](../design/facilities/03-concurrency/effects.md#thin-callable-targets)
for its signature-wide conservatism and remaining erased-callable limits.
Static integer domains were completed by [B95](language-and-tooling.md#b95-std-limit-domains).

### Dataframe byte-view inputs (issue #657)

`std.dataframe.lit_binary` accepts `&[u8]` from literals, owned buffers, byte
lists and subviews. Its C `strided` adapter passes address, length and stride
1 directly, with no Plenty-side payload allocation. Binary-column constructors
pass their packed `Bytes` through that adapter too, removing the additional
`list[u8]` payload copy. Polars still owns a native copy; extraction still
builds independent owned buffers. Mutable byte views remain unsupported by
this adapter. See [binary payloads](../design/stdlib/dataframe-types.md#binary-payloads)
and [C borrowing](../design/facilities/04-interop/borrowing-text.md#strided-numeric-lists-and-byte-views).

## Windows x86-64 dataframe packaging

The experimental Windows release builds the Polars dataframe engine natively
and bundles `plenty_dataframe_v1.dll` with its licenses. The compiler generates
an MSVC import library from the selected DLL, copies the DLL beside ordinary
applications, and includes validated metadata and notices in `--dist` output.
Release and portability workflows exercise Parquet queries and app relocation;
Windows execution still requires native runner validation. Linear algebra and
macOS engine packaging remain outside this delivery. See the
[engine installation reference](../design/stdlib/dataframe-engine.md#windows-application-linking).

## macOS ARM64 dataframe packaging

The experimental Apple silicon release builds the Polars dataframe engine for
Apple M1 and macOS 11, and bundles `libplenty_dataframe_v1.dylib` with its
metadata and licenses. The dylib has a versioned `@rpath` identity; applications
find their sibling copy through `@loader_path`. Ordinary output copies the
dylib through a temporary sibling and atomic rename, preserving already-open
engine files when rebuilding (issue #678); `--dist` adds validated notices and
checks library identity and checksum. Selected-engine validation errors include
the supported rebuild command.
Packaging checks native dependencies, exports, deployment minimum and ad-hoc
code signature. Native workflows exercise Parquet queries, moved installations
and application-only relocation; native execution remains for CI to verify.

The shared native archive writer now gives libraries and notices mode 0644,
resolving issue #677 for both Windows and macOS bundles. Broader B18.7 library
qualification, linear algebra on these platforms and signing/notarization
remain outside this delivery. See
[macOS engine installation](../design/stdlib/dataframe-engine.md#macos-application-linking).


## Explicit child-file inheritance

B43/B55, issue #777: [`std.process.Command.inherit_file`](../design/stdlib/process.md#explicit-file-inheritance)
owns close-on-exec copies for a selected child, and
[`std.fs.lock.Lock.inherit_into`](../design/stdlib/fs/lock.md#ownership-and-release)
keeps advisory locks alive after runner death. Child-only flag changes preserve
parent flags and isolate concurrent spawns; commands cap selection at 64 files,
retain copies for reuse, and release them through `clear_inherited_files()` or
drop. Focused tests cover concurrent selection, source close, failed exec,
allocation failure and lock lifetime across runner death. Arbitrary descriptor
remapping and standard-stream redirection remain B43 scope.
