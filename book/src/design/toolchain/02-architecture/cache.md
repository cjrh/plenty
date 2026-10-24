# Compile cache

Every compile used to lower every function a program reaches, including all
of an imported `std` module, from IR to machine code. Most of that code is the
same from one program to the next, and Plenty's objects are reproducible
([native emission](emission.md)): the same input, compiler, and target give
the same bytes. The compiler therefore keeps a local, content-addressed cache
of compiled functions and reuses an entry whenever a function it is compiling
has one.

Reusing functions still runs the frontend and builds every function's IR:
with every function cached, these were 45% of a warm build of a large program
(issue #304). The cache therefore also keeps each program's
[object](#program-objects), which a build whose sources have not changed uses
without running the frontend or code generation.

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
result (rewriting GOT loads of local data, see
[symbol references](emission.md#symbol-references), then placing bytes and
relocations in the object) is the same whether they came from Cranelift or the
cache. Entries hold Cranelift's output before that rewrite. The compiler's build identity, such as a
Git hash, is deliberately not part of the key: it would empty the cache at
every commit without excluding any stale entry. A change to Plenty's code
generation changes the IR it builds, and so the key.

Every damaged or stale entry is a miss, never an error: an entry carries a
checksum of its payload, and Cranelift rejects entries written by another
version of itself.

Plenty buffers the key's `Hash` byte stream before feeding it to SHA-256.
The byte stream and serialized entries remain compatible with the pinned
Cranelift version (0.137.0); unit tests compare buffered keys to Cranelift's
keys and exercise entry exchange in both directions. Dependency upgrades
must retain these checks: the upstream key implementation is not a stable
serialization format.

Within one compilation, each distinct key has a lazily initialized memory
entry. Concurrent requests for that key share its disk read, decoding, or
compilation; unrelated keys can initialize in parallel. The memory entry
keeps only machine bytes, alignment, and unresolved relocations. Relocations
are resolved using each current function's symbol table and function ID;
source locations and other parameterized Cranelift metadata are not reused.
Cranelift's optional cache-check mode continues through its original path,
including recompilation and comparison of full metadata.

This saves duplicate decoding at the cost of retaining all distinct machine
code and relocation buffers until the compilation's cache is dropped. There
is no separate memory eviction limit. In-memory reuse counts as a hit, but
adds no disk bytes read; the reported time saved is still an estimate.

## Program objects

A build of an application (`plenty FILE`, `--compile`, `--emit-object`,
`plenty build`, `plenty run`, and `plenty test`) looks up its program before
loading any module. An entry holds the object the build wrote and the inputs
it read. Library builds, `--check`, and legacy-syntax programs do not use
program entries.

The entry's name is a SHA-256 of:

- the cache format version, the compiler's and Cranelift's versions, and the
  compiler's identity: its executable's path, size and modification time;
- a digest of every standard module's text, which the compiler embeds;
- the target, the optimization level, and whether the build has `plenty test`
  hooks or fuzzing coverage;
- the request: the canonical entry file and the package graph (each package's
  canonical root, identity, library and dependencies), and the `main` that
  `plenty test` appends; or, for a source compiled from memory, its text.

Unlike a function's stencil, the frontend's output depends on the compiler's
whole code, so the key names the compiler binary rather than its version. A
rebuilt compiler has a new modification time, so every build after a compiler
change, including a change to `std`, misses. The binary is not hashed: that
would cost about as much as the warm build it saves.

The name does not cover the imported modules, which a build finds only by
parsing. The entry records them instead: each source file read, with a
SHA-256 of its text, the package whose root it resolves against, and where
each of its imports resolved. A lookup uses the entry only if every recorded
file still has that text and every recorded import still resolves to the same
file or standard module. Resolving an import again repeats every existence
check the loader made, so a new file that would make an import ambiguous, or
change which file it names, is noticed though the program never read it. The
package checks of a build, such as reserved names and overlapping roots, run
again too. Anything else (a missing, damaged, or undecodable entry, an edited
file, a changed resolution) is a miss: the build runs and its object replaces
the entry. Each request therefore keeps one entry, its latest build's.

The first build of a request stores only an empty entry, a marker that it
was built; a later build of it stores its object. So the third build of an
unchanged program is the first hit, and after an edit, the first rebuild
stores and the next one hits. A program built only once, such as each
temporary program of the test suite, costs a few bytes rather than its
object. Without markers, one run of the test suite stored 3,870 objects
(1.3 GB, beside 194 MB of compiled functions); with them it stores 438
objects and 2,925 markers (96 MB).

The frontend and code generation are a function of these inputs: no phase
reads the environment, the clock, or any file other than the modules it
loads, and the object is the same for any number of cores. A program hit
therefore writes the bytes a build would. Only a successful build stores an
entry, so a program hit never hides a diagnostic. Linking still runs on a
hit.

Every edit to a module the program reads misses, including a change to a
function body that no other module's code depends on: the frontend checks a
program's modules together (see [frontend](frontend.md)), so there is no
smaller unit to keep. See [later phases](#later-phases).

`--no-program-cache` (`CompileOptions::program_cache = false`) keeps
reusing compiled functions but neither uses nor records program entries.
`compile_bench` uses it, so its builds measure the frontend and code
generation. With `--cache-stats`, the line begins `program hit` or
`program miss` when the build consulted the program cache; a hit reports no
function hits or misses.

## Location and controls

| Setting | Effect |
|---|---|
| `PLENTY_CACHE_DIR=DIR` | Use `DIR`. The default is `$XDG_CACHE_HOME/plenty`, else `~/.cache/plenty` |
| `PLENTY_CACHE=off`, `--no-cache` | Compile without the cache; the flag applies to one compiler command |
| `--no-program-cache` | Reuse compiled functions, but not [program objects](#program-objects) |
| `PLENTY_CACHE_LIMIT=SIZE` | Keep at most `SIZE` bytes (`512M`, `2G`, or a byte count); the default is 1 GiB |
| `--cache-stats` | Print whether the program hit, function hits and misses, bytes read and written, and an estimate of the time saved |
| `plenty cache stats` | Print the directory, its entry count (functions and programs) and size, and the limit |
| `plenty cache clean` | Remove every entry |

The same directory also holds the automatic linker probe's results under
`DIR/linker-probe/` ([Native linking](../01-building/linking.md)); they are not
cache entries, and `plenty cache clean` leaves them.

Function entries live under `DIR/fn/` and program entries under
`DIR/program/`, sharded by the first byte of the hash. Each is written to a
temporary name in its shard and renamed, so compilers running at once in
several processes and threads share one directory without a lock and never
read a partial entry. Two of them missing the same function write the same
bytes; two building one program at once each write a complete entry, and the
later rename wins.

A compile that wrote entries trims the directory to the limit when no sweep
ran in the last hour (`DIR/sweep` records the last one), removing the least
recently used entries of either kind first, by the later of each file's
access and modification time. Linux updates access times lazily, so recency
is coarse, and no hit writes anything. A program entry with an object is
about the size of that object (1.7 MB for a program that imports
`std.http.server`).

If the directory cannot be created or written, the compiler prints one warning
and compiles without the cache for the rest of the process; a read-only
directory still serves hits. A program entry that cannot be written is
skipped without a warning, so a read-only directory that serves every
function does not warn on each build. The cache changes no diagnostic: code
generation never reaches a program with errors, and only successful builds
store programs.

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

The [bounded module-cache investigation](../../../development/module-cache-investigation.md)
records current edit profiles, a scalar native-artifact spike, and a runnable
counterexample to signature-only invalidation. It changes no cache behavior.

**Per-module artifacts.** Program objects make a no-change rebuild nearly
free, but any edit runs the whole frontend again, `std` modules included. A
per-module cache would keep, for each module:

- its *interface*: the resolved public declarations other modules can see
  (function signatures, class, enum and protocol definitions with their type
  parameters and layouts, aliases, exports, and access rules), including any
  fact that lowering infers from a body and a caller relies on, such as the
  fields a function returns;
- its *body*: the lowered, checked operations of its non-generic functions,
  with its string literals as a table of its own, and the generic functions
  and data it defines, in the resolved form that instantiation needs;
- the generic instances it requests, with their type arguments.

Its key would be a SHA-256 of the module's text, the compiler's identity and
settings, and the interface hashes of its direct imports. A body edit then
rebuilds only the edited module; dependents rebuild only when an interface
they import changes. An instance of a generic depends on the generic's body,
not just its interface, so an instance would be keyed by the defining
module's body hash as well. A program build would combine the modules'
bodies and instances in a fixed order, deduplicating instances and
renumbering strings, so the object stays byte-identical to an uncached build.

The frontend cannot produce these yet: it checks all modules as one program.
Type resolution creates generic data instances in a table shared by every
module, lowering interns strings and records instances and returned fields in
tables shared by every body, and signatures are collected into one
whole-program table (see [frontend](frontend.md)). The operations and types
also have no serialized form; nominal types are graphs with weak edges. The
work is: give lowering a per-module view built from imported interfaces, make
instance and string tables per module and merge them afterwards, serialize
operations and types, and keep program-order diagnostics.

No smaller artifact pays for itself (issue #317). Loading a stored tree
allocates every node, as building it does, so a copy of a phase's result
bounds what loading it costs:

| Phase output (release compiler, system allocator) | Building it | Copying it |
|---|---|---|
| Syntax trees of the 1,000-unit program | 63 ms to parse | 40 ms |
| Operations of the 1,000-unit program (7,001 functions) | 109 ms to lower | 15 ms |
| Operations of a program importing `std.http.server` (605 functions) | 19 ms to lower | 0.5 ms |

Stored syntax trees would save at most a third of parsing, about 5% of a
warm build after an edit. Name resolution rewrites those trees in place and
costs less than a copy of them (20 ms for the 1,000 units split into ten
modules). Only lowered operations are much cheaper to load than to build, so
a module artifact must hold them, and that needs the lowering work above. A
program that is one module, such as `compile_bench`'s, would need the same
artifacts per function.

For `std`, lowering only the functions a program reaches would cut work in
every build, cold or warm: in a program that imports `std.http.server` and
prints, the calls in its operations reach 64 of the 222 `std` functions
(drop hooks and thread, executor and sort adapters, not counted, reach more),
and in `examples/http_server_bench.plenty` 155 of 230.

Native emission now omits unreachable standard definitions *after* checking
(issue #400; see [emission](emission.md)). This reduces object size and native
work without a cache redesign. It does not implement selective frontend
lowering or module artifacts. The [module-edit probe](../../../development/module-edits.md)
checks unchanged, body, interface and generic-body edits against uncached objects
and provides fresh-process time and peak-memory measurements for further work.

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

A program hit (issue #317; `plenty --compile`, release compiler, function
cache warm in both cases, paired runs on a loaded host):

| Program | Function cache only | Program hit |
|---|---|---|
| `compile_bench` 1,000 units as a file | 3.59 G instructions, 1.72 s CPU, 0.46 s wall | 0.22 G, 0.24 s, 0.08 s |
| `import std.http.server` and `print` | 0.75 G, 0.40 s, 0.13 s | 0.19 G, 0.23 s, 0.07 s |

What remains on a hit is mostly linking. A build after an edit misses; it
also hashes the sources and writes the entry, about 1% more instructions for
the 1,000-unit program.

A build that misses its program entry, as after an edit, runs the whole
frontend even with every function cached (`--emit-object`, profile build,
eight cores):

| Program | Build | Parsing | Name resolution | Lowering | Object emission |
|---|---|---|---|---|---|
| 1,000 units in ten modules | 267 ms | 41 ms | 20 ms | 103 ms | 90 ms |
| `import std.http.server` and `print` | 51 ms | 9 ms | 3 ms | 17 ms | 19 ms |

With `plenty --compile`, these builds take 3.16 G and 0.62 G instructions,
and 3.08 G for the 1,000 units as one module.
