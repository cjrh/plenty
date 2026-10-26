# Standard library scope

> **Status: Partly implemented.** The [standard library reference](../design/stdlib/index.md)
> owns the shipped contracts: collections, text/data formats, files/processes,
> streams, networking, TLS, HTTP clients/servers and helpers, crypto, SQLite,
> logging, signals, and testing. The catalogue below is the original scope,
> not a list of modules still missing.
> Issue #49 retains the original catalogue and rationale; it is closed by
> decomposition, not implementation. Remaining delivery is recorded only in
> [backlog B38–B70](../backlog.md). Module names and unimplemented signatures
> remain design candidates. This page adds no language syntax or runtime API.

The goal is useful CLI/data tools and network services without third-party
application packages. Python's library breadth and Go's combined HTTP
client/server scope motivated issue #49; compatibility with their APIs is not
a requirement. A production HTTP server remains an explicit bundled-library
goal, with [separate acceptance criteria](completed/standard-http.md).

## Boundaries

Keep the prelude small and import larger facilities under `std`. Ship source
and approved native dependencies with the matching toolchain; ordinary builds
must import the library offline after installation or relocation. Importing an
unused module must not impose unrelated backend/linker requirements on a program.
Issue #45 continues to own package distribution, manifests and project commands.

Ordinary algorithms, containers, parsers, routing and policy belong in checked
Plenty source. Standard imports pass the same ownership and worker-effect
checks as user modules. Runtime boundaries cover allocation, handles/syscalls,
clocks, entropy, sockets/polling, native synchronization and approved bindings.
Reuse maintained TLS/crypto, SQLite and suitable compression/math primitives;
record backend versions, licenses, supported targets and update responsibility.
[Plenty or native](#plenty-or-native) sets when a native library is chosen.
Library purity must not weaken correctness or security.

Current support is x86_64 Linux GNU. Lossless Linux `Path` and `list[u8]` binary
transport have shipped; neither is a portable path abstraction or compact byte
view. Further target support requires explicit platform contracts and tests.

## Plenty or native

Write a std module in Plenty unless a native library is clearly better:
cryptography, a large established engine, or performance the compiler cannot
reach. A Rust crate named in an issue is a reference for API and test vectors
unless the issue makes that case. Issue #505 records this decision.

Plenty code costs less in an executable:

- **Removal.** Emission drops unreachable std functions one at a time before
  native code is generated ([issue #400](../design/toolchain/02-architecture/emission.md)),
  and release builds substitute small leaf functions at the call site
  ([issue #363](../design/toolchain/02-architecture/emission.md#call-site-substitution)).
  Native runtime code is removed only at link time, by
  [`--gc-sections`](../design/toolchain/01-building/linking.md#external-build-systems),
  and only when nothing kept still calls it. A central runtime dispatcher
  keeps every operation it can reach (issue #501).
- **Allocation failure.** Plenty code reports it as `AllocError` by
  construction. A wrapped crate must be checked, since Rust's global allocator
  aborts on failure. rustls is the recorded exception
  ([`std.tls` costs](../design/stdlib/tls.md#costs)); new wrappers get none.

A native wrapper must:

- report allocation failure as `AllocError`, through a fallible interface or
  a caller-supplied allocator, as `zlib-rs` does for
  [`std.compress`](../design/stdlib/compress.md#supply). If the crate cannot,
  write the needed part in Plenty instead;
- give each operation its own runtime entry symbol, as the `std.crypto`
  digests have, so the linker drops what a program does not call;
- add no case to a shared runtime dispatcher;
- when large, ship as a separate engine needed only by programs that import
  it, like the [`std.dataframe` engine](../design/stdlib/dataframe-engine.md)
  (issue #504).

### Open module issues

The policy applied to issues #461–#499. "Decide" means the issue settles the
choice with a measurement or allocation check before implementation.

| Issue | Module | Choice | Reason |
|---|---|---|---|
| #461 | `std.uuid` | Plenty | Byte formatting over `std.secrets` and `std.time` |
| #462 | `std.xml` | Plenty | Streaming parser like `std.json`; `quick-xml` allocates infallibly |
| #463 | Time zones | Plenty | Package; packages carry only Plenty source |
| #464 | AEAD, signatures, key exchange | Native | Cryptography; `ring` is already linked for `std.tls` |
| #465 | Unicode segmentation | Plenty | Package; generated tables |
| #466, #467 | WebSocket | Plenty | Framing over `std.http` and `std.tls` |
| #468 | Server-Sent Events | Plenty | Text encoding over a response stream |
| #469 | `std.diff` | Plenty | Myers and patience are short algorithms over lists |
| #470 | `std.markdown` | Plenty | Parser and renderer; `pulldown-cmark` allocates infallibly |
| #472 | User directories | Plenty | XDG rules over `std.env` |
| #473 | SHA-1 | Native | Cryptography; RustCrypto `sha1` allocates nothing, like `sha2` |
| #474 | `Bytes.find_bytes` | Native | SIMD search the compiler cannot match; `memchr` allocates nothing |
| #475 | `std.toml` edits | Plenty | `std.toml` is Plenty; `toml_edit` allocates infallibly |
| #476 | `std.image` | Decide | Codecs are large engines, but `image` allocates infallibly; separate engine if native |
| #477 | Progress bar | Plenty | Terminal drawing over `std.io` |
| #478 | `std.shlex` | Plenty | Small tokenizer |
| #480 | `std.memmap` | Plenty | A few system calls behind small runtime entries |
| #481 | `std.bigint` | Decide | Measure Plenty limb arithmetic against native call cost; `num-bigint` allocates infallibly |
| #482 | LRU cache | Plenty | `dict` and `std.deque` |
| #483 | BLAKE3 | Native | Cryptography with SIMD; confirm hashing allocates nothing |
| #484 | inotify | Plenty | A few system calls behind small runtime entries |
| #485 | `std.mime` | Plenty | Parsing and a lookup table |
| #486 | `std.graph` | Plenty | User data on nodes and edges; algorithms over lists |
| #487 | System information | Plenty | Text files under `/proc` and `/sys` |
| #488 | `std.jwt` | Plenty | Composes `std.json`, `std.encoding` and `std.crypto` |
| #489 | base32 | Plenty | Same as the existing codecs |
| #490 | Linear algebra | Native | Large engine with SIMD kernels; separate engine; check `faer` allocation first |
| #491 | Brotli | Native | Large codec; the `brotli` crate takes an allocator, so failure is reported |
| #492 | Optimization | Decide | Depends on #490 and #493; the algorithms may be Plenty over native linear algebra |
| #493 | `numeris` evaluation | Decide | Its result decides whether #490 and #492 use `faer` |
| #494 | Statistics | Plenty | Formulas over lists; sampling keeps `std.random`'s stable sequence |
| #495 | Key-value store | Decide | Large engine with its own file format; `std.sqlite` already covers transactions and ordered keys |
| #496 | Sketches | Plenty | Small algorithms over arrays and hashes |
| #497 | Xor filters | Plenty | Short construction over a fixed key set |
| #498 | Cuckoo filters | Plenty | A small hash table |
| #499 | `std.template` | Plenty | Already chosen; `tera` renders from serde values |

Issue #471 designs derived serialization and makes no implementation choice.
Issue #479 is closed.

## Shared contracts

Establish binary buffers, partial-progress I/O, contextual errors and monotonic
deadlines before consumers invent incompatible alternatives. Preserve typed
Result errors, explicit fallible allocation and existing cooperative
cancellation. Specify EOF, short progress, interruption, close/flush errors and
cleanup after partial I/O, cancellation or allocation failure.

Structural protocols are statically dispatched. A proposed API must not assume
associated types, existential storage, reflection or automatic codec derivation.
Use implementable generics, explicit error/value enums and typed codecs.
Consuming/borrowing, copied/streamed and in-place/allocating operations need
distinct documented contracts. Binary data is not UTF-8 text; filesystem paths
are not URLs; simulation randomness is not cryptographic entropy.

Every adopted module publishes its scope, errors, allocation/copy costs,
ownership/thread safety, supported platforms and complexity where relevant.
Add small reference pages, runnable tutorial examples and SUMMARY entries with
the implementation. Use local fixtures, fixed seeds, fake streams/clocks,
failure injection, external format fixtures and retained parser fuzz corpora.
Test Plenty libraries through `plenty test` when issue #45 supplies its runner,
alongside native primitive/lifecycle tests.

## Capability checks and further breadth

The initial CLI/data check reads arguments/environment, loads TOML/JSON, walks
files, processes CSV and reports contextual errors/structured logs. Existing
math, text primitives, lossless paths, walking, sorting and deque support count
toward it. It does not wait for a network execution model. The service check
fetches verified HTTPS with cancellation/bounded streaming, persists records
in SQLite and serves a routed JSON API meeting the HTTP acceptance gates.

Accurate summation/norms, float decomposition/adjacent values and special
functions need numerical contracts; basic math has already shipped. Optional
scope still includes timezone/calendar data, statistics, decimal/big-integer/
complex arithmetic, UUIDs, MIME types, HTML escaping/context-aware templates,
WebSockets, diff tools, terminals and signals. Evaluate each against a concrete
use case before adding an implementation issue/backlog entry. HTTP/2 is a
follow-on protocol candidate; HTTP/3 needs a separate decision.

GUI toolkits, mail/FTP stacks, interpreter/debugger internals, arbitrary runtime
loading and pickle-style serialization are outside the initial catalogue.
YAML, an ORM and a complete web framework also require their own case.
Closing the umbrella does not broaden these commitments or claim delivery.
