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
Library purity must not weaken correctness or security.

Current support is x86_64 Linux GNU. Lossless Linux `Path` and `list[u8]` binary
transport have shipped; neither is a portable path abstraction or compact byte
view. Further target support requires explicit platform contracts and tests.

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
