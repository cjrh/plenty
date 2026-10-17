# Compiled-function cache

Every compile used to lower every function a program reaches, including all
of an imported `std` module, from IR to machine code. Most of that code is the
same from one program to the next, and Plenty's objects are reproducible
([native emission](emission.md)): the same input, compiler, and target give
the same bytes. The compiler therefore keeps a local, content-addressed cache
of compiled functions and reuses an entry whenever a function it is compiling
has one.

## What is cached

The unit is one compiled function: its machine code, alignment, and
relocations, as Cranelift serializes them. Generic instantiations are
functions of their own, so they are cached like any other.

Cranelift compiles a function's *stencil*: the IR without the identities of
the external symbols it names. The IR refers to a callee, a type descriptor,
or a string literal by an index into a small table belonging to the function;
the table is applied to the compiled code afterwards, and the placing thread
resolves the function's requested symbols in the object. A cached entry holds
the code for the stencil, with relocations against those indices. On a hit
the compiler still builds the function's IR, so the table and the requested
symbols are this program's, and applies them to the cached code. Two programs
whose functions have the same stencil but print different types, call
different functions, or sit at different source positions therefore share one
entry and each get their own symbols.

## The key

An entry's name is a SHA-256 of:

- Cranelift's key for the function: the stencil, the target triple, and every
  ISA flag, which includes the optimization level, the register allocator
  (chosen per function, see [emission](emission.md)), the verifier setting,
  and the frame and probe settings;
- the cache format version;
- the compiler's version and Cranelift's version.

The compiled code for a stencil depends on nothing else: the compiler's own
work ends with the IR, which the key covers, and what it does with the
result (placing bytes and relocations in the object) is the same whether they
came from Cranelift or the cache. The compiler's build identity, such as a
Git hash, is deliberately not part of the key: it would empty the cache at
every commit without excluding any stale entry. A change to Plenty's code
generation changes the IR it builds, and so the key.

Every damaged or stale entry is a miss, never an error: an entry carries a
checksum of its payload, and Cranelift rejects entries written by another
version of itself.

## Location and controls

| Setting | Effect |
|---|---|
| `PLENTY_CACHE_DIR=DIR` | Use `DIR`. The default is `$XDG_CACHE_HOME/plenty`, else `~/.cache/plenty` |
| `PLENTY_CACHE=off`, `--no-cache` | Compile without the cache; the flag applies to one compiler command |
| `PLENTY_CACHE_LIMIT=SIZE` | Keep at most `SIZE` bytes (`512M`, `2G`, or a byte count); the default is 1 GiB |
| `--cache-stats` | Print hits, misses, bytes read and written, and an estimate of the time saved |
| `plenty cache stats` | Print the directory, its entry count and size, and the limit |
| `plenty cache clean` | Remove every entry |

Entries live under `DIR/fn/`, sharded by the first byte of the hash. Each is
written to a temporary name in its shard and renamed, so compilers running at
once in several processes and threads share one directory without a lock and
never read a partial entry. Two of them missing the same function write the
same bytes.

A compile that wrote entries trims the directory to the limit when no sweep
ran in the last hour (`DIR/sweep` records the last one), removing the least
recently used entries first, by the later of each file's access and
modification time. Linux updates access times lazily, so recency is coarse,
and no hit writes anything.

If the directory cannot be created or written, the compiler prints one warning
and compiles without the cache for the rest of the process; a read-only
directory still serves hits. The cache changes no diagnostic: code generation
never reaches a program with errors.

The test suite compiles thousands of programs; the workspace's
`.cargo/config.toml` points `PLENTY_CACHE_DIR` at `target/plenty-cache`, so
every test run of a worktree shares one cache and none touches the user's
`~/.cache`. Tests that exercise the cache use directories of their own.

## Trust

The cache holds machine code that the compiler will link into programs. A
local directory is trusted like the compiler binary itself: anyone who can
write it can already replace the compiler. The checksum detects damage, not
tampering. Sharing a cache between users, or over a network, needs the trust
model described below, which this phase does not implement.

## Later phases

These are design notes, not implemented behavior.

**Skipping IR building and lowering.** The cache saves Cranelift's work but
still parses, checks, and lowers every module. A per-module cache keyed by the
module's source hash and the interface hashes of its dependencies (public
signatures and layouts, so a body change does not invalidate dependents) would
make a no-change rebuild nearly free and give incremental compilation for user
code.

**Remote and shared cache.** The same keys over HTTP to a content-addressed
store would serve CI and teams. It must be optional and opt-in, readable by
default and written only by trusted builders such as CI, perhaps through the
Bazel Remote Execution API's cache protocol or a minimal GET/PUT. Hashes are
not a trust boundary: an entry is machine code that will run. Trust only
configured servers, and verify entries by recompiling a sample in CI.

## Measurements

Release compiler, eight cores, warm page cache (issue #144):

| Program | No cache | Cold cache | Warm cache |
|---|---|---|---|
| `import std.http.server` and `print` (576 functions) | 0.14 s wall, 0.67 s CPU, 65 MB | 0.15 s, 0.75 s, 68 MB | 0.06 s, 0.14 s, 24 MB |
| `compile_bench` 1,000 units (7,002 functions, 16 MB of entries) | 0.86 s wall, 4.6 s CPU, 102 MB | 0.80 s, 4.1 s, 103 MB | 0.47 s, 1.5 s, 94 MB |

A one-line edit to the 1,000-unit program misses one function. A cold run
already has hits: functions of one program with the same stencil share an
entry.
