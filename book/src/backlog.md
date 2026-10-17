# Backlog

This is the single maintained list of planned language, compiler, runtime, and
tooling work. It replaces the separate roadmap and next-work lists. Entries are
candidates, not promises about syntax or delivery dates. Allocation-free ranges
and concrete generator frames are implemented independently.

Use the [implementation status](design/toolchain/02-architecture/support.md) to see what
works today, the [reference](design/index.md) for its exact contract, and the
[proposals](proposals/index.md) for design rationale and alternatives. A proposal's
stages describe technical dependencies, not a second priority list.

## Current position

Reconciled against local `main` at `736fe80` on 2026-10-12, merged local PRs,
the reference and tutorial, and the shared issues register. **B34's bounded
cleanup and review batch is complete.** Its ownership, diagnostics, compiler
performance, collection conveniences, and documentation fixes have landed.
Remaining limitations have explicit homes below; completing this review does
not mean all candidates or every concurrency diagnostic are implemented.

### Next batches

1. **Standard-library foundations.** The first B39 compact-bytes slice (issue
   #76), B41 error context (issue #78), and B42 time/deadlines (issue #79) have
   landed, as have B40 binary streams (issue #77) with direct `Bytes` I/O, B49
   heaps and generator adapters (issue #86), and B54 binary encodings (issue
   #91). B43 environment/processes (issue #80), B50 randomness/secrets (issue
   #87), B51 JSON (issue #88), B53 CSV (issue #90), and B60 URLs (issue #97)
   have also landed. B39 index syntax, iteration, and `fill` landed (issue
   #76); only sub-range view values remain, blocked on B11. Bulk byte scanning
   landed (issue #115) and is used by `std.csv`, `std.json`, and `std.url`.
2. **A focused CLI/data workflow.** Select a concrete use case and its acceptance
   checks, then compose the needed CLI parsing, encodings, codecs, filesystem,
   and test-support slices. B44/B51–B55/B57 are candidates, not one mandatory
   umbrella delivery. Networking and HTTP follow their prerequisites and B66
   qualification rather than displacing this sequence.

Issue severity still takes precedence when a new defect needs attention. Claims
and implementation/review status belong in `issues.db`; this sequence assigns no
ownership. Table row order below remains a catalogue, not a second priority list.

### Deliberate deferrals

- **B19:** issue #59 remains open for cache/streaming costs and the remaining
  serial parsing, lowering, and IR construction. Parallel function checking and
  table/pass reductions have landed; further architectural optimization needs
  measurements and a focused batch, and does not keep B34 open.
- **B22:** join/shutdown checks for sending workers are implemented (issues
  #12/#74). Scoped joins also check receiving workers against surviving senders
  (issue #108), and blocking future-result checks are implemented (issue #109).
  Other documented
  limits remain explicit in the [channel](design/facilities/03-concurrency/channels.md#what-the-rule-does-not-decide)
  and [executor](design/facilities/03-concurrency/executors.md#shutdown-and-live-receivers)
  contracts; these checks do not establish deadlock freedom.
- Broader concurrency mapping/effects, streaming results, rendezvous, larger or
  send-side selection, automatic parallelization (B24), and additional backends
  (B20) remain deferred. Stored-reference relationships (B11), broader callback
  erasure (B32), and automatic deep recursive operations (B33) remain separate
  candidates requiring their own contracts and acceptance checks.

### Scope of follow-on work

**B11 — broader reference relationships.** Borrowed enum matching and payload
loans are implemented. Remaining slices include stored references, relationships
among multiple reference parameters, and more precise collection loans. Preserve
owner/variant stability and reject references into temporary storage as this
subset expands; evaluate additional precision against compile-time cost.

**B22 — useful native concurrency.** Scoped native threads, structural eligibility,
reachable worker/destructor effect checks, and atomic ownership are implemented.
Jobs may borrow owners or consume a concrete owned closure; failed consuming
starts return the unstarted job without allocation. Results transfer at join.
Positive-capacity MPMC channels are implemented with recoverable construction,
blocking/nowait transfer, failed-send ownership, and endpoint guards. The initial
Python-style executor now supports bounded submission, result retrieval, eager
ordered list/range mapping, pending cancellation, and context-managed shutdown.
Fallible ordered mapping is implemented with deterministic earliest-input errors
and partial-result cleanup; see [parallel operations](design/facilities/03-concurrency/parallel-map.md).
Fixed adjacent-pair tree reduction is also implemented, including allocation-free
empty/singleton paths and fallible scratch storage. Cooperative cancellation
tokens, timed waits, and two-channel receive selection complete the committed batch.
Broader iterator/closure mapping, streaming results, richer certified effects/types,
zero-capacity rendezvous, and broader selection are deferred beyond B34.
Thread/task creation and queue allocation must report failures; specify bounded
queues/backpressure, failed-send ownership, joining on every exit, cooperative
cancellation, and partial-result cleanup. Executor result handles do not require
async/await. The initial explicit parallel operations are complete;
automatic parallelization remains B24. Prefer library APIs over new syntax, with
compiler support for ownership checks and runtime support for native threading.
Scoped generic tasks need not wait for heterogeneous closure storage in B32.

**B32 — broader callback storage.** Owned concrete reusable/consuming environments
work in classes, tuples, lists, dictionaries, and user enums; see the
[storage decision](proposals/stored-closures.md). Explicit fallible
[BoxedCallable](design/language/06-functions/05-boxed-callables.md) implements
heterogeneous owned shared-call storage (issue #25). Mutable, consuming, borrowed,
generator-containing, and worker-bound erased modes remain unsupported. Evaluate
additional representations against concrete registry/event-handler needs and
specify invocation, move/drop, allocation failure, and capture lifetimes before
adding types. Escaping borrowed captures depend on B11. C ABI callbacks and
foreign-library lifetimes remain B28.

## Work items

The next batches above select the next work. This table catalogs remaining
work, including deferred extensions to completed batches; its row order does
not set priority. Detailed syntax is still open unless an agreement is cited.
Issue references identify related open concerns or completed fixes as stated;
they do not approve every suggested API. Status, reproductions, discussion, and
ownership remain in the shared `issues.db`.

| ID | Work | Scope / dependency |
| --- | --- | --- |
| B11 | Broader borrowing | Borrowed enum matching and payload loans complete. A `mut` reference binding is reassigned only through itself (issue #1). Remaining: stored references, relationships among multiple reference parameters, more precise collection loans, retargeting a reference binding to another borrow of the same owner, and field-precise loans through `mut` reference bindings. Borrowed iteration now consistently yields references (issue #30, also B12). Mutable matching of copyable enum payloads (issue #41) is complete through B35. B30 completed ordinary nested writes; disjoint-index precision remains here. Evaluate precision against compile-time cost; the current single-parameter returned-borrow rule is documented in the [reference](design/language/05-ownership/04-returned-references.md). |
| B12 | Practical I/O and iteration | Text-file iteration and a consistent borrowed-iteration rule are implemented (issue #30, also B11); borrowed temporary source owners survive through their loop scope (issue #75). Allocation-free `enumerate`, `zip`, and `reversed` loop/comprehension sources and short-circuiting boolean `any`/`all` reductions are implemented for issue #35 (also B38). Remaining: explicit binary buffers and read/write, buffering, reusable iterator/view protocols, and first-class adapters. Lossless platform paths and binary argument/I/O transport are concrete gaps (issue #61); coordinate their representation with B38's filesystem work. Follow familiar Python conventions where they fit ownership and Result-based errors. |
| B13 | Public allocator control | Global/default and per-container selection, allocator state/lifetimes, and buffer provenance. Internal object allocator identity already exists. Consider bounded/inline-capacity storage separately. [Memory design](proposals/memory-parallelism-and-simd.md). |
| B14 | Typed error composition | Preserve details across multiple error types through explicit unions/conversions. `Failure` already supplies deliberate erasure; it does not replace recoverable typed errors. Allocation-free `Option.unwrap_or` and explicit `Option.ok_or` conversion to typed Result are implemented for the first slice of issue #35 (also B38). Additional Option/Result conveniences remain separate API work. |
| B16 | Additional inference and value conveniences | Consider expected-result inference, partial explicit type arguments, and contextual literals across generic arguments. B35 completed inline storage (issue #17); classes still move. Mutable component borrows work for every `mut` tuple, with independent copied storage; direct indexed assignment stays unsupported (issue #68). Numeric augmented assignment evaluates each target index once and protects its owner during right-side evaluation (issue #32). The bare numeric cast checks its range and `T.wrapping_from` names truncation (issues #39 and #65). `T.try_from` returns a single-variant `RangeError`, with explicit mapping to `ParseError` documented (issue #66), and unsigned negation is rejected even for literals such as `-0u8` (issue #67). Related gaps: a saturating conversion, which has no spelling now that out-of-range float-to-integer casts trap, and typed composition between parsing and conversion errors (B14). Standalone unit bindings/parameters/storage, nested unpacking, and slice syntax/steps also need concrete use cases and contracts. These remain candidates, not automatic extensions of today's inference rules. |
| B17 | Modules, project tooling, and packaging | File imports, visibility, and embedded `std` distribution are implemented. `std` is reserved and its Plenty source ships inside the compiler. [Packages](design/toolchain/01-building/packages.md) are done (issue #45, local PRs #124 and #130): the `plentyc`/`plenty` split, strict `plenty.toml`, Git and path dependencies pinned in `plenty.lock`, offline and locked modes, workspaces, version requirements, `new`/`init`/`add`/`fetch`/`update`/`graph`/`check`/`build`/`run`/`test`/`clean`, per-target build reuse, and `plentyc --package-graph`. Candidates: a shared per-user Git cache, semver selection over tags. Native artifacts use a fixed CPU baseline, deterministic archives, and runtime build-path remapping (issue #58); the [reproducibility contract](design/toolchain/01-building/linking.md) requires the same compiler, source, options, and native linking environment. Remaining: separately cached module objects (per-module incremental compilation), re-exports, and import-cycle policy. Preserve deterministic name resolution and standalone compilation. Coordinate further std modules with B38. [Module design](proposals/entrypoints-modules-and-tutorials.md) |
| B18 | Windows and additional targets | Target-specific runtime support, linker-driver conventions, object/executable naming, and native CI. Depends on target validation and packaging work; changing the linker alone is insufficient. |
| B19 | Measured codegen and storage improvements | Benchmark dense-match dispatch before adding jump tables. B35 completed inline storage; measure natural field packing and inline copies beyond the current 16-byte slots and runtime copy calls. Tail `list.pop()` now avoids survivor relocation, with suffix-only repair for other positions (issue #52). Remaining concerns include packed numeric buffers, code size, and compilation/link latency. Collection access/removal, equality, and string indexing improvements (issues #18–#21), linear declaration checking (issue #53), linear checking of a `match` over a large enum (issue #71), collection type facts combined from cached element facts (issue #6), parallel native function emission without release verification (issue #55), and single-pass register allocation for functions with more than 2,048 branch arguments, which keeps emission of one very large function close to linear at a cost in that function's code size and speed (issue #56), are complete. The independent operation check runs on all cores, and native emission builds and compiles IR on all cores with byte-identical objects for any worker count (local PR #131); name resolution, frame validation, tail-call preparation and adapter scans also run on all cores (local PR #136). Issue #59 is closed: declarations are resolved once (local PR #158); parallel parsing (pieces or modules), speculative parallel lowering and parallel declaration phases were measured and not adopted (small gains, higher peak memory). Remaining compile-time work is serial efficiency: freeing at exit, allocation per `Ty` clone, parser vector capacity. IR construction is 7% of emission CPU time and limits emission only above about fourteen cores. Compiler peak memory is about 2.3 KB per source line when checking and 2.8 KB when emitting an object, after removing duplicate bodies and syntax trees and releasing tokens and operation bodies as they are used (issue #54), and the remaining peak (the whole syntax tree beside the declaration tables when lowering starts, and every function's operations when emission starts) needs lowering and emitting one function at a time; the backtracking register allocator's quadratic block-parameter handling is not yet reported to regalloc2, and a fix there would let the 2,048 limit go; and checking many small functions costs more per function as the program grows, because whole-program tables keyed by function name outgrow the processor caches; pre-sized tables, then fewer per-function tables and whole-program passes, removed a quarter of that cost and half of its growth (issue #57). Checking is now linear in instructions, and std imports no longer enable per-function whole-program work (issue #57); time per function still rises about 20% from 16k to 256k functions from cache misses, because each phase walks every function, and needs interned names or the same per-function batch processing. |
| B20 | Optional GCC backend | Evaluate the proposed backend with a small feasibility experiment before committing to another backend or common IR. Cranelift remains the current backend. [GCC proposal](proposals/codegen-gcc.md). |
| B21 | Documentation authoring | Evaluate independently runnable literate lesson sources that generate mdBook pages if they improve authoring. Current Markdown lessons are already executable and tested; do not create another manually maintained copy. [Documentation design](proposals/entrypoints-modules-and-tutorials.md). |
| B22 | Threads, channels, and parallel operations | Committed batch complete: eligibility/effect checks, scoped native threads, consuming jobs, bounded channels, executors, explicit fallible mapping/tree reduction, cancellation tokens, timed waits, and receive-pair selection. Completed cleanup: scoped named workers accept owned arguments, and map/map_result share eligible owned reusable immutable closures (issue #28). Mutable, one-shot, borrowed, generator-containing, and indirect map callbacks remain unsupported. Completed cleanup under B34: a join of a sending task is rejected while a matching receiver is live (issue #12). Completed cleanup under B34: a pool shutdown after submitting a sending job is rejected while a matching receiver is live (issue #74). Broader mapping, richer effects, rendezvous, and broader selection are deferred beyond B34. Preserve fallible submission and deterministic shutdown. [Threads](design/facilities/03-concurrency/threads.md); [channels](design/facilities/03-concurrency/channels.md); [executor](design/facilities/03-concurrency/executors.md); [parallel operations](design/facilities/03-concurrency/parallel-map.md); [cancellation](design/facilities/03-concurrency/cancellation.md); [timed waits and selection](design/facilities/03-concurrency/timed-selection.md). |
| B23 | SIMD | Define vector values, contiguous numeric storage, supported operations, and scalar fallbacks. Ordinary generic lists are not automatically packed native arrays. [SIMD design](proposals/memory-parallelism-and-simd.md). |
| B24 | Automatic parallelization | Explicitly deferred beyond B34. Require evidence that effects, cleanup, allocation failures, and result ordering remain correct. A runtime thread-count setting alone does not establish those guarantees. |
| B27 | Expanded C representations | Export text/byte buffers and borrowed views, broader typed errors and recoverable returned ownership, and explicit class-method adapters. Keep allocator provenance, output initialization, and error-path transfer visible. Initial class factories support AllocError, which reports allocation of the boxed handle. See [owned exports](design/facilities/04-interop/owned-exports.md). |
| B28 | Broader library loading and lifecycle | Generate runtime loaders directly from extracted contracts without original export source; improve error detail and binding tooling for third-party C libraries. Unloading, retained/foreign-thread callbacks, C record layout, and header-assisted bindings need separate contracts. Current loaders keep mappings resident and require their creating thread. [Runtime loading](design/facilities/04-interop/loading.md). |
| B29 | Further generic constraints | Callable bounds on data are implemented. Consider protocol bounds on data declarations, generic methods satisfying protocol requirements, combined bounds, and protocol composition when concrete library APIs need them. Generic aliases and enum constructor inference also remain candidates. Instantiation failures are reported at the requiring use with a note at the body (issue #40); checking bodies against declared bounds, so errors independent of the type arguments stay at the definition, remains a candidate. Preserve explicit lookup and bounded specialization. See [generic data](design/language/07-generics/data.md) and [parameterized protocols](design/language/07-generics/protocols.md). |
| B32 | Broader callback storage | Owned concrete reusable/consuming environments and their worker eligibility are implemented. Explicit fallible shared-call erasure is implemented (issue #25). Remaining: mutable/consuming erased modes and generator-containing environments in heap owners. Escaping borrowed storage depends on B11. See scope above. |
| B33 | Deep operations on recursive values | Recursive source storage is implemented under B10. Replace native recursive copy/format traversals with iterative fallible work storage before enabling them; select an explicit bounded-memory contract for structural equality and membership without adding hidden allocation failure. Preserve failure cleanup, shared-string ownership, and float/NaN semantics. [Design rationale](proposals/recursive-data.md). |
| B36 | Named arguments and defaults | Implemented for named functions, methods and constructors: static name binding, source-order argument evaluation, fresh per-call defaults, callee-local error propagation and nonescaping default loans. See [call rules](design/language/06-functions/01-signatures-and-calls.md) and [rationale](proposals/named-arguments-and-fresh-defaults.md). Callable values/builtins remain positional; protocol-constrained generic bodies conservatively reject named method calls. Remaining candidates: finer protocol receiver tracking and deliberate builtin names. Structured initialization for large records remains separate. Splats, variadics and `/`/`*` markers remain in the [older unpacking proposal](proposals/keyword-arguments-and-unpacking.md). |
| B37 | Ordering beyond `sort()` | `list.sort()` orders integers, floats, `str`, and tuples of those in place; `swap` and assignment to several targets are complete (issue #31). Lexicographic string comparisons are implemented (issue #22). Allocation-free unstable `sort_by_key` with shared element borrows and recomputed keys, positional descending bools, and fallible natural-order `sorted` copies are implemented (issue #64). Tuple relational operators matching the total sort order are implemented (issue #111). Remaining extensions: cached or fallible keys, stable sorting, comparison functions, and class ordering protocols. Preserve no hidden allocation. |
| B38 | Standard library scope | Issue #49 is split into B39–B70, with [catalogue rationale](proposals/standard-library-scope.md) and [HTTP acceptance](proposals/standard-http.md). [std.math](design/stdlib/math.md), [std.path](design/stdlib/path.md), [std.fs](design/stdlib/fs.md), [std.deque](design/stdlib/deque.md), [std.error](design/stdlib/error.md), [std.bytes](design/stdlib/bytes.md), [std.time](design/stdlib/time.md), [std.io](design/stdlib/io.md), [std.encoding](design/stdlib/encoding.md), [std.collections](design/stdlib/collections.md), [std.iter](design/stdlib/iter.md), [std.url](design/stdlib/url.md), [std.csv](design/stdlib/csv.md), [std.random](design/stdlib/random.md), [std.secrets](design/stdlib/secrets.md), [std.json](design/stdlib/json.md), [std.env](design/stdlib/env.md), and [std.process](design/stdlib/process.md) are implemented; reuse completed issues #22/#29/#31/#33/#34/#35/#39/#60/#61 rather than duplicating them. Remaining numerical candidates: accurate summation/norms, float decomposition/adjacent values and special functions, each with numerical contracts/tests. Defaults and arbitrary constant expressions are language candidates; reusable iterator contracts remain B12. Portable paths remain B12; packages/runner remain B17, issue #45. Further library breadth needs a concrete use case and its own scope before scheduling. |

### Stdlib-driven language ergonomics

| ID | Work item | Scope |
| --- | --- | --- |
| B71 | Explicit error composition | `std.error.map_error` implements explicit result mapping, with eleven HTTP/JSON adapters using it. An isolated [typed-stream prototype](proposals/error-composition.md#prototype-findings) exercises progress, retry, and fixed-length HTTP body composition; production stream/HTTP migration and other framing modes remain. Generic dependency depth and total resource budgets are now separate (issue #141), allowing the broader adoption. Independently assess [finite error sets](proposals/error-sets.md); runtime error-protocol objects require a separate representation decision. |
| B72 | State and ownership ergonomics | Explore [typestate at owned boundaries](proposals/typestate-and-owned-transitions.md) and payload enums for runtime state machines. Existing-code cleanup candidates: issue #134 (fetch placeholder head), issue #135 (empty-string copies), issue #136 (dummy mutable Results). Keep changes independent and coordinate HTTP overlap; no blanket conversion or general partial-move feature approved. |

### Standard library delivery slices

These entries replace issue #49's umbrella scope. Each linked issue records
acceptance checks, errors, allocation/ownership contracts and dependencies.
Dependencies identify technical prerequisites; row order is not a priority or
approval of a proposed API. All slices remain unimplemented unless explicitly
recorded otherwise. B39/B41/B42 are foundational contracts; a useful CLI/data
slice composes them with B40/B44/B51/B52/B53/B55/B57 and B17 tooling. HTTP must
pass B66 before it is described as production-ready. Libraries use checked
Plenty; approved platform/backend boundaries remain narrow.

| ID | Work / issue | Scope / dependency |
| --- | --- | --- |
| B39 | Compact bytes — issue #76 | [std.bytes](design/stdlib/bytes.md) owning buffers, checked ranges, explicit copies, `&Bytes`/`&mut Bytes` views under the ordinary loan rules, index syntax with list index rules, iteration, and native `fill` are implemented. Remaining: first-class sub-range view values (needs B11 stored references). Slice syntax is deliberately omitted: it would hide a copy. Bulk scans landed with issue #115 and are used by `std.csv`, `std.json`, and `std.url`; short-token parsers are bounded by per-call cost. Direct `Bytes` I/O landed with B40. B12; #61 transport keeps `list[u8]`. |
| B40 | Binary streams — issue #77 | Complete: [std.io](design/stdlib/io.md) delivers statically dispatched reader/writer/seek/close contracts with explicit partial progress, `read_exact`/`write_all`/`read_up_to`/`copy_all`, fixed-capacity buffering, flush-versus-close errors, and file, process-stream and memory adapters over `Bytes`. Unscheduled candidates: nonblocking descriptors, vectored I/O, peeking, type-erased streams; text and formatting over streams are B45/B47. |
| B41 | Error context — issue #78 | [std.error](design/stdlib/error.md) is implemented: `Context[E]` with operation/path/typed cause, explicit conversion, descriptions and stderr reports with nonallocating fallbacks, and module conventions. Unit Results are supported (issue #110). Remaining: OS description text and entrypoint rendering of a `Context`. B14. |
| B42 | Time/deadlines — issue #79 | Complete: [std.time](design/stdlib/time.md) delivers checked `Duration`, monotonic `Instant`/`Deadline`, wall `Timestamp`, and sleep. Unscheduled candidates: `Duration`/`Deadline` overloads of timed waits, calendar and formatting support. |
| B43 | Environment/processes — issue #80 | Complete: [std.env](design/stdlib/env.md) lossless variable reads, snapshot and working directory; [std.process](design/stdlib/process.md) argument-vector spawning with no implicit shell, per-child env/cwd, piped streams, deadlock-free bounded output collection, exit/signal status, termination and explicit reaping. Remaining candidates under B38: deadline waits through pidfd and cancellation tokens; redirecting a stream to a file or another child and merging stderr into stdout; a reaping policy for dropped children (needs owner destructors); process groups. |
| B44 | CLI parsing — issue #81 | Implemented: checked `std.cli` schemas, typed options/positionals, repeatable values, `--`, help/errors and lossless path arguments. Depends on B41. |
| B45 | Formatting — issue #82 | Implemented: [std.fmt](design/stdlib/fmt.md) provides static user display/debug contracts, scalar and callback aggregate formatting, bounded streaming with partial progress, and fallible strings over B40 writers. Widths count bytes; recursion is explicit and bounded. No dynamic format strings, automatic derive, locale or precision controls. |
| B46 | Structured logs — issue #83 | Done: `std.log` leveled events with typed fields, explicit `Logger[S]` over caller-chosen sinks (logfmt `TextSink` over any writer, bounded `MemorySink`, nonblocking `ChannelSink` with `drain`), fixed/wall/off clocks and bounded lines. Candidates: JSON-lines sink, sampling/rate limits, per-module thresholds, span context. |
| B47 | Text/Unicode — issue #84 | Implemented: bounded reusable [builders/join/scalar wrapping](design/stdlib/text.md) and [Unicode 17 NFC/NFD/NFKC/NFKD](design/stdlib/unicode.md), with combining-class/Mark classification. Reuses B39 bytes and #22 text primitives. Grapheme/display-width wrapping and broader classification remain outside this slice. |
| B48 | Regex — issue #85 | Done: [std.regex](design/stdlib/regex.md) linear-time Pike VM in Plenty with explicit pattern, nesting, repetition, instruction, memory, work, output and match-count limits; text and bytes; captures, iteration, split, replacement. Candidates: a faster engine (lazy DFA or one-pass) and literal prefilters, full Unicode case folding, script and general-category properties. |
| B49 | Collections/iterators — issue #86 | [std.collections](design/stdlib/collections.md) binary heaps (`Heap`, comparator `HeapBy`, `drain`) and [std.iter](design/stdlib/iter.md) `map`/`filter`/`take`/`fold`/`try_fold` over owned generators are implemented. Remaining: adapters over borrowed collections and a user iterator protocol (B12, needs B11 stored references); tuple `<` so `Heap` accepts tuples (issue #111); further adapters need a use case. |
| B50 | Randomness/secrets — issue #87 | Complete: [std.random](design/stdlib/random.md) provides caller-owned seeded xoshiro256** generators with a published version-stable sequence, unbiased ranges, shuffle, index sampling, and byte fill; [std.secrets](design/stdlib/secrets.md) provides OS-backed fill, byte/hex/base64url tokens, and unbiased integers over one `getrandom` primitive with no fallback. Remaining candidates under B38: jump/split streams, distributions and float ranges, weighted choice and element sampling, scratch-free secret integers, a zero-on-drop buffer. |
| B51 | JSON — issue #88 | Complete: [std.json](design/stdlib/json.md) is a strict RFC 8259 codec in Plenty: resumable event `Decoder`, validating `Encoder`, and a `Json` value tree, all under `Limits` (depth, input, output, duplicate keys), with located `JsonError`s and stream integration through `std.io`. Remaining candidates under B38: bulk scanning outside string runs, pretty-printing, JSON Lines, derived typed codecs. |
| B52 | TOML — issue #89 | Done: `std.toml` strict TOML 1.0 parser and writer over a value tree, bounded by depth/input/output limits, with located errors. Remaining: comment-preserving editing for manifest tooling stays B17 (#45); an independent conformance check (toml-test or a reference crate); TOML 1.1 when final. |
| B53 | CSV — issue #90 | Complete: [std.csv](design/stdlib/csv.md) streaming `Reader`/`Writer` over `std.io` with a reusable `Record`, explicit `Dialect` (delimiter, quote, newline, quoting, width policy, field/record/field-count limits), strict RFC 4180 quoting, and positioned `CsvError`. Remaining candidates under B38: lenient parsing and recovery after syntax errors, header mapping and typed columns, non-UTF-8 encodings, multi-byte delimiters, and a throughput benchmark. |
| B54 | Binary encodings — issue #91 | Complete: [std.encoding](design/stdlib/encoding.md) strict hex/base64/base64url and explicit-endian `u16`..`i64` packing over `std.bytes`. Candidates under B38: bulk native codec/packing primitives for throughput, streaming codecs, float packing, varints, base32. |
| B55 | Filesystem helpers — issue #92 | Implemented: descriptor-rooted no-follow operations, anonymous temporary file publication, empty-only temporary-directory cleanup, bounded copy/delete and byte glob filtering. Linux O_TMPFILE/procfs and trusted-parent rename limits are explicit in the reference. |
| B56 | Concurrency modules — issue #93 | Implemented: explicit thread/sync/channel/concurrent imports preserve builtin identities and join/ownership/effect guarantees. B42 satisfied; additional primitives still need a use case. |
| B57 | Library test support — issue #94 | Done: [std.test](design/stdlib/test.md) assertions explain failures on stderr; `Scratch` directories, seeded `rng()`, `FakeClock` and `FakeStream` (short reads/writes, injected I/O errors) fixtures; `plenty test` runs module-level `test_*` functions in parallel processes with filters, `--jobs`, `--timeout`, per-test `TMPDIR` cleanup, `--list`, `--no-capture` and `--seed`; assertion failures report the caller's position, and `plenty test` executables can fail allocations and file I/O on demand (local PR #142). |
| B58 | Trusted I/O effects — issue #95 | Done: every native primitive declares a worker effect (`src/effects.rs`), with no std-wide exemption; native handles cross threads under the loan rules; only `set_current_dir` is denied. [std.ready](design/stdlib/ready.md) provides deadline- and token-bounded readiness waits: bounded blocking waits on explicit workers, cancellation within a 10 ms slice. Tokens now wake waits directly and selectors wait on many handles (B59). |
| B59 | Sockets/DNS — issue #96 | Done: [std.net](design/stdlib/net.md) TCP/UDP sockets and addresses over `std.ready` (local PR #135); tokens wake waits directly, `std.ready` selectors wait on many handles, `resolve` is deadline- and token-bounded on a helper thread, `NetError.HandleLimit` reports the descriptor limit, and accept-queue bounds are measured (local PR #140). Unix-domain sockets declined for now (see net.md). x86-64 Linux only. |
| B60 | URLs — issue #97 | Complete: [std.url](design/stdlib/url.md) provides strict RFC 3986 absolute URL parsing and validated formatting without normalization, component-aware percent encoding, and query pairs. Remaining candidates under B38: relative reference resolution, normalization/equivalence helpers, IDNA, form encoding, and use of the `std.bytes` bulk scans. |
| B61 | TLS — issue #98 | Done: [std.tls](design/stdlib/tls.md) TLS 1.2/1.3 client and server streams over std.net with vendored rustls 0.23 + ring, verification against PEM or system roots, std.net deadlines and cancellation, typed certificate errors (local PR #145); client certificates, ALPN, SNI certificate selection, version and cipher suite choice, CRL revocation, and selector-driven `try_` methods (local PR #147). Declined: recoverable allocation inside rustls (no fallible API), session resumption and 0-RTT, OCSP. |
| B62 | HTTP/1.1 messages — issue #99 | Done: [std.http](design/stdlib/http.md) reads and writes HTTP/1.1 heads and streams `Content-Length`, chunked and close-delimited bodies over `std.io`, with explicit limits, strict rejection of ambiguous framing, and a mutation-tested regression corpus (local PR #139). |
| B63 | HTTP client — issue #100 | Done: [std.http.client](design/stdlib/http-client.md) (HTTP/1.1) with pooling, per-phase and caller deadlines, cancellation, bounded redirects with cross-origin credential stripping, an explicit forward proxy, and replay of idempotent requests only (local PR #146); `https` through `std.http.https` with mandatory verification, proxy `CONNECT` tunnels, and streamed request bodies with `Expect: 100-continue` (local PR #154). Declined: a thread-shared pool until std.sync has a mutex; a client cookie jar and content decoding (callers handle `Set-Cookie` and decode with std.compress under their own limit). |
| B64 | HTTP server — issue #101 | Done: [std.http.server](design/stdlib/http-server.md) with ordered method/path routing, a fixed worker pool behind a bounded admission queue (503 when full), per-phase deadlines, `std.log` events and counters, a `Transport` protocol, and graceful shutdown (local PR #143); idle keep-alive connections parked on a `std.ready` selector (`idle_connections`) and HTTPS through `std.http.tls` with handshake deadlines counted from the accept (local PR #148). |
| B65 | HTTP/deployment helpers — issue #102 | Done: [std.http.body](design/stdlib/http-body.md) (bounded forms, JSON, multipart uploads to unnamed temporary files), [std.http.cookie](design/stdlib/http-cookie.md) (validated `Set-Cookie`, signed cookies with key rotation), [std.http.proxy](design/stdlib/http-proxy.md) (trusted reverse proxies, reverse-proxy and direct-TLS deployment guidance) and [std.http.files](design/stdlib/http-files.md) (static files confined to a `Root`, conditional requests) (local PRs #144, #152). Later: `Range` requests and directory indexes for `std.http.files`. |
| B66 | HTTP production qualification — issue #103 | In progress: [qualification](design/stdlib/http-qualification.md) suites (corpus and mutations, curl/TLS/client interop, slow clients, shutdown under load, allocation failure, descriptor/heap leaks, relocated compiler, backend linkage) and published baselines (local PR #157). Done since: RFC 9112 `Host` and absolute-form rules (issue #131), request heads and TLS handshakes parked instead of holding workers (issue #132), `std.signal` drain on `SIGTERM`/`SIGINT` (issue #133), lingering close after refusals (issue #138); no qualification gate fails. Not yet labelled production-ready: coverage-guided fuzzing, a reverse-proxy deployment test, installed-toolchain workflows and HTTPS benchmarks remain. Depends on B63/B65/B57. |
| B67 | Curated crypto — issue #104 | Done: `std.crypto` SHA-256/SHA-512 digests, HMAC, and constant-time `equal`/`verify` over vendored RustCrypto crates. Remaining: key derivation (HKDF/PBKDF2) and encryption need separate API/use-case review. |
| B68 | Compression — issue #105 | Done: `std.compress` gzip/zlib/DEFLATE buffers and `std.io` streams over vendored zlib-rs, with output limits, gzip member/trailing-data policy, checksum errors, fixed per-stream memory and no linkage when unused. Candidates: gzip header access, zstd, tolerance of trailing zero padding; ZIP/TAR stay B69. |
| B69 | Archives — issue #106 | Done: `std.archive` TAR (ustar/pax/GNU) and ZIP (ZIP64, stored/DEFLATE) streaming readers and writers, and confined extraction through a staging directory under an explicit `Policy`. Candidates: link creation under an explicit policy, mode/time restoration, streaming ZIP entry writes with data descriptors. |
| B70 | SQLite — issue #107 | Done: [std.db.sqlite](design/stdlib/sqlite.md) with vendored SQLite 3.54.0, typed prepared statements, owned transactions that roll back on drop, copied-out values and an explicit length limit. Candidates: route SQLite allocation through the runtime allocator (`SQLITE_CONFIG_MALLOC`) for accounting and failure injection; user-defined functions; backup API. |

## Reconciled completed work

Issue #61's initial lossless Unix transport is implemented with owned `list[u8]`,
explicit UTF-8 conversion, raw arguments, binary file helpers, and NUL stream
records. The finder now exercises those contracts with `--null`. Under B38/B12,
compact byte buffers (B39), platform path abstractions (B12), buffered streams
(B40), and broader `std` distribution (B17) remain future work. Directory
traversal and metadata from issue #60 are implemented in `std.fs`.

Integer bitwise operators, checked shift counts, and explicit wrapping
add/subtract/multiply are implemented for issue #33. The numeric reference and
tutorial describe their precedence and width rules; this closes that B38
language prerequisite.

These old backlog entries are closed, not additional tasks. The linked reference
pages own the current details; this table records why they disappeared from the
active list.

| Previous item | Resolution |
| --- | --- |
| B34: Post-concurrency cleanup and review | Completed the bounded review through local main `736fe80`. Ownership, inline storage, tail-call cleanup, typed error reporting, diagnostics, sorting/container conveniences, compiler scaling, reproducible artifacts, and tutorial/reference restructuring have landed. The final reconciliation removes stale status text, records the remaining channel wait gaps as issues #108/#109, and gives the next batches an explicit order. Residual compiler performance (#57/#59) remains B19; further concurrency diagnostics remain B22. Book/reference/example validation accompanies this closing change; ongoing audits use the [development workflow](development/workflow.md). |
| B15: Enum-pattern shorthand | Patterns take generic arguments from the matched value, nest, combine with `\|`, and match integer, `str`, and `bool` literals; `match` accepts those scalar types (issue #36). Omitting a user-defined enum's qualifier was declined: a bare name is always a binding and a qualified name is always a variant. `case _ as name:` binds the whole matched value in a final case, moving an owned scrutinee and reborrowing a referenced one (issue #114); a bare name as a case's pattern stays an error. A `match` block is the value of a binding, a single-target assignment, or `return` (issue #118); `if` blocks are not values in those positions. See [matching](design/language/04-expressions/matching.md). |
| B35: Inline values | Classes, tuples, and enums store their fields inline; their storage construction allocates nothing and needs no `?` (issue #17). Argument expressions and custom initializers retain their own failures, so a fallible initializer still makes its constructor return Result. Recursive types use `Box[T]`, which converts to its content wherever the content's type is required; recursion without a heap boundary is rejected. C export handles box their instance. See [classes](design/language/03-types/classes.md) and [recursive data](design/language/03-types/recursive-data.md). |
| B10: Recursive source types | Self/mutual recursive classes and enums, aliases, and finite generic instances reach themselves through boxes and collections (originally heap records; see B35). Finite compiler graphs avoid type-table leaks; native source tests drop 100,000-node class and enum chains without allocation on a 256 KiB stack. Runnable lessons, module visibility, and C/Plenty library consumers cover the initial subset. Borrowed matching is B11; automatic deep operations are B33. |
| B09: Initial generic data types and richer protocols | Concrete generic classes/enums, IntType data bounds, function/constructor inference, inferred/explicit generic methods, parameterized structural contracts, and protocol-driven inference are implemented. Native allocation-failure/drop checks and specialization reuse/limits cover the initial subset. Extensions are B29; recursive storage was completed under B10. |
| B08: Function values and multiline closures | Thin functions, reusable/consuming concrete environments, explicit captures, factories, signature inference, `Callable`/`OnceCallable` constraints, inline sums, captured generator frames, and checked lifecycle cleanup are implemented without implicit allocation. B31 reviewed the surface and simplified its teaching path; captured-environment storage remains B32 and stored/escaping borrows remain B11. See [closures](design/language/06-functions/03-closures.md) and [consuming closures](design/language/06-functions/04-consuming-and-stored-closures.md). |
| B30: Nested assignment | Existing list/dictionary/field slots update without enclosing copies or allocations. RHS and saved indices precede address resolution; tests cover resizing, aliases, failure cleanup, and owner replacement. |
| B31: Callable simplicity audit | The initial review recorded use-case rationale and a simpler teaching path; ordinary generic records demonstrate stateful registries without another callable type. The follow-up agreed to keep `Callable` and `Closure`, explain mismatches through generic callback guidance, and display closure source locations (issue #24 under B34). |
| B07: Initial runtime library loading | CLI/API generation emits self-contained typed loaders; exact discovery metadata and all symbols are checked before returning a table. Scalar/borrow/Result adapters and owned factories/transfers preserve signatures, destruction, and instance provenance. Allocation-free LoadError, injected failures, private-address checks, and the executable library tutorial cover the initial resident implementation. [Runtime reference](design/facilities/04-interop/loading.md). |
| B06: Initial C library exports | Static/shared packaging, scalar and Result adapters, owned factories, matching destruction, shared/mutable and consuming handles, precise C contracts, embedded/extractable interfaces, SHA-256 link guards, explicit binary verification, and staged publication are implemented. Native callers and injected failures validate cleanup. [Library reference](design/facilities/04-interop/exports.md). |
| B05: Ownership-aware C adapters | Scalar/pointer borrows and explicit UTF-8/C-string adapters integrate with normal borrowing. Private opaque pointers in classes provide owned handles and matching destruction; C fixtures verify partial acquisition, allocation errors, and conditional/unconditional transfer. Mutable/returned byte buffers remain outside this subset. See [adapters](design/facilities/04-interop/borrowing-text.md). |
| B04: Initial C imports | Trusted `.plentyi` declarations support sized C scalars and nominal opaque pointers. Native C fixtures cover static/shared linking and actual ABI calls; visibility and Plenty layouts remain separate. See [C interfaces](design/facilities/04-interop/trusted-declarations.md). |
| B03: Target compatibility | Native compilation validates the supported x86_64 Linux GNU target, packaged runtime triple, and ISA pointer width. Explicit target selection rejects incompatible layouts before source loading/emission; runtime extraction includes target metadata. See [runtime packaging](design/toolchain/02-architecture/runtime.md). |
| B02: Object-file output | CLI/API application object emission and matching runtime extraction work without a linker on PATH. External-driver tests link and run single-file and imported programs. See [execution commands](design/toolchain/01-building/commands.md). |
| B01: Linker selection | CLI `--linker` / `--link-arg` and Rust `CompileOptions` select a cc-compatible driver without shell parsing. Native and mock-driver tests cover argument fidelity, failure diagnostics, and temporary cleanup. See [execution commands](design/toolchain/01-building/commands.md). |
| B25: Allocation-free generator frames | Generator calls return concrete inline frames directly. Consumers specialize by producer identity; factories and standard sums preserve that identity. Native tests disable heap allocation across creation, moves, calls, nested frames, resume, and cleanup. Recursive inline layouts are rejected. See [native generators](design/language/09-iteration/03-generators.md). |
| B26: Allocation-free ranges | Range construction now returns a copyable inline value directly. Calls, returns, standard sums, indexing, membership, and repeated traversal need no range allocation. Stored ranges live in their containing owner's storage. See [collections and iteration](design/facilities/01-builtins/collections.md). |
| String/heap reclamation | Owned values and strings are reclaimed by the Rust runtime, with deterministic cleanup. See [ownership](design/language/05-ownership/index.md) and [destruction](design/language/05-ownership/06-destruction.md). |
| Length-aware strings | Strings already store explicit lengths and support embedded NULs. The original `strlen`-based premise is obsolete; adopting a two-word source value is not required to resolve it. |
| Precompiled runtime archive | The runtime archive is built with the compiler and embedded for linking. It is not rebuilt from C source for each program. See [runtime](runtime.md). |
| Foundation batch | Main/imports/visibility, fallible allocation APIs, borrowing, tuples/items, typed ranges, generic functions, and structural protocols have implemented paths. See the [current status](design/toolchain/02-architecture/support.md). |
| Allocation syntax and error convenience | Allocating operations return Result by default; `?`, explicit success values, and `Failure` replace the old `try`/`try_` alternatives and repetitive unwraps. |
| Generic argument inference | Calls infer concrete type parameters from arguments; explicit arguments remain available. Unit type arguments work in function specializations, including `Result`/`Option` payloads (issue #110); standalone unit parameters/storage remain B16. See [generic functions](design/language/07-generics/index.md). |
| Catch-all matching | `case _:` already handles remaining enum variants. See the [enum lesson](tutorial/06-data/06-enums.md). |
| mdBook migration | Reference and tutorial pages live under `book/src/`, are listed in `SUMMARY.md`, and tutorial examples run in tests. Literate-source generation is the separate B21 candidate. |

## Keeping this list useful

- Add, prioritize, split, or mark active work here. Keep IDs stable; link design
  detail instead of copying a task list into a proposal, tutorial, or reference.
- When selecting a candidate, settle its scope and acceptance checks. The table
  order does not approve every proposed API or require all later candidates.
- Before adding syntax or builtins, show the user problem, why ordinary library
  code cannot solve it, and the ownership, allocation, compilation, and teaching
  costs. Ease of implementation alone is not a reason to enlarge the language.
- On completion, remove it from the active queue and update implementation
  status, the relevant reference pages, and runnable lessons in the same change.
  Retain a short completion note here only when it explains a retired entry.
- Use the [development workflow](design/../development/workflow.md) for shared testing,
  documentation, and performance expectations. Those are ongoing practices,
  not separate feature priorities.
- Historical proposals may contain old syntax and dependency sketches. Their
  introductory notices must make that clear; current priorities belong here.

Async/await, a JIT, and an interpreter remain out of scope, rather than waiting
at the bottom of this backlog.
