# Standard library and OS semantics

> Part of the [cross-platform assessment](../cross-platform.md).
> These are proposed contracts to resolve before implementing native ports.

## Paths, text, and filesystem guarantees

`std.path.Path` stores native Linux bytes, exposes `as_bytes`/`from_bytes`, and
checks an absolute path by its initial slash. `std.env` and filesystem/process
helpers also exchange native bytes. Windows uses UTF-16 OS strings, drive roots,
UNC paths and device names; replacing separators or converting invalid text
lossily would change filenames and could undermine confinement.

Choose a native-path representation with lossless round trips before Windows
implementation. Recommended direction: an opaque OS-native `Path` and explicit
text conversion; keep raw Unix-byte constructors/accessors platform-specific,
and provide an explicit Windows wide-unit boundary if needed. Consider a
documented lossless internal encoding, but never describe that encoding as UTF-8
text or portable filesystem bytes. This is a public API migration requiring
compatibility decisions, tutorial examples and tests, not merely a private FFI
change. Reject unsupported conversions explicitly. Process arguments and
environment names/values need the same policy; source code remains UTF-8.

Preserve lexical operations separately from filesystem resolution. Test drive
relative paths, UNC/device prefixes, reserved names, trailing dots/spaces,
case-sensitive and case-insensitive volumes, Unicode normalization, long paths,
symlinks and Windows reparse points. macOS paths are not automatically
case-sensitive just because they are Unix paths. Archive member names are a
portable format namespace; extraction into native paths must reject traversal
and collisions under the destination filesystem's rules.

`aggregates/filesystem.rs` uses descriptor-rooted no-follow operations and Linux
`O_TMPFILE` plus `/proc/self/fd` publication. Darwin needs an alternative
temporary-file/publication strategy. Windows needs owned directory/file handles,
reparse-point policy, sharing/deletion rules and an honest atomic replacement
contract. Security must be evaluated under concurrent rename/link replacement,
not only clean-directory tests. Never replace rooted operations with unchecked
string concatenation. If a promised no-follow/atomic operation has no adequate
backend, return a documented unsupported result. HTTP static files, multipart
uploads and archive extraction inherit these requirements.

## Handles, errors, readiness, and networking

The current runtime uses Rust `File`/Unix descriptors for several resource kinds.
Windows `HANDLE`, Winsock `SOCKET` and C-runtime descriptors are distinct; an
integer cast and a common `close` are not a resource abstraction. Introduce
typed owned native resources behind existing language ownership, with distinct
close/read/write/wait behavior and cancellation-safe lifetimes. Audit runtime
tags and compiler intrinsics that currently assume a file-shaped resource.

`std.net` and `std.report` classify numeric Linux errno values. Move portable
classification to the native boundary and preserve original OS domain/code
for diagnostics. Decide how existing public `IoError`/`NetError` variants expose
that information without losing partial progress. Darwin errno, Win32 errors
and Winsock errors are not interchangeable. Broken pipe, interrupted operation,
would-block, timeout and cancellation must retain their specified distinctions.
Review error formatting (`strerror_r` variants included), newline behavior,
binary/text console I/O and SIGPIPE suppression.

| Facility | Current assumption | Proposed port boundary |
| --- | --- | --- |
| `std.ready`, cancellation | epoll/poll and Linux eventfd | Darwin kqueue or qualified poll/wakeup implementation; Windows design for sockets **and** pipes/files plus wakeable cancellation |
| `std.net`, DNS | Linux socket constants and hand-written structs; pthread resolver helper | OS bindings, Winsock initialization/error mapping, bounded resolver lifetime and platform address conversion |
| `std.signal` | Linux signal delivery on both admitted architectures | Darwin signal backend; explicitly define Windows console/service shutdown capabilities instead of pretending POSIX signals exist |
| `std.process` | Linux implementation, Unix status/signals and pipes | Native argument-vector spawn, inheritance control, Windows quoting/handle lists, termination/reaping and bounded output |
| `std.secrets` | Linux `getrandom`; unsupported elsewhere | Fallible OS entropy primitive per platform, preserving no-weak-fallback guarantee |
| `std.time` | Rust clocks plus deadline contracts | Verify monotonicity, precision, overflow, suspend behavior and timed waits per backend |

Windows IOCP is completion-based, while the public ready API is readiness-based.
Choose the bridge deliberately: a readiness abstraction for supported resources,
or a completion-backed adapter that owns buffers and preserves lifetimes. A
socket-only `select`/poll replacement does not implement waiting on process
pipes and cancellation tokens. Account for handle limits, stale registrations,
EOF/hangup, cancellation races and deadline rounding. macOS also needs tests
for event removal and descriptor reuse; sharing a Unix API shape is insufficient.

## Threads, TLS, and higher layers

`threads.rs` directly declares pthread functions and fixes task storage to a
machine-word thread identifier. Use target-specific thread creation/join and
entry ABI while preserving pinned caller-owned storage, fallible startup and
join-before-reclaim. Rust's boxed thread startup is a useful semantic reference,
not an automatic replacement for Plenty's allocation contract. Review stack
sizes, TLS initialization and cleanup, atomics/alignment and memory ordering.
AArch64's weaker ordering makes native concurrency stress especially important;
channels and executors must not rely on x86 behavior.

Rustls protocol logic is broadly reusable, but the vendored ring build, entropy,
socket progress, readiness and certificate paths must first work. Preserve
verified TLS, caller-selected trust roots, mutual TLS, ALPN, SNI and revocation
contracts. Native OS certificate stores would be a separate trust-policy
feature, not an implicit change in a port. Requalify HTTP pooling, shutdown,
deadlines and file serving after lower layers pass; parser tests alone do not
prove an HTTP server port.

Collections, JSON/TOML/CSV, URL parsing, encoding and much of regex/compression
should need fewer OS changes, but still require target execution, allocation
failure and integer/Unicode compatibility checks. SQLite requires its correct
OS VFS/build configuration, locking, durable-write and path tests. Math/float
edge behavior, endian packing and CPU-dispatched crypto/compression need their
own fixtures. Avoid blanket gating that turns a green test suite into an
undocumented subset of the standard library.

A limited preview may explicitly omit unqualified modules. Before a general
release, publish a capability matrix and ensure unavailable operations are
rejected predictably without unresolved symbols, compiler panic, unsafe fallback
or silent loss of guarantees. The current reference remains authoritative until
each implemented port updates its contracts and learner-visible examples.
