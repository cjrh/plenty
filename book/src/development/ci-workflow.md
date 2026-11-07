# CI and repository configuration

For contributor commands, see the [development workflow](workflow.md).

## Rust build profiles

Both workspace crates (`plenty` and `plenty-runtime`) use Rust edition 2024.
The embedded runtime archives built directly by `build.rs` use the same edition;
vendored third-party crates retain their upstream editions.

Development and release builds disable incremental compilation. Workspace
sources (`plenty` and `plenty-runtime`) retain line tables for backtraces with
file names and line numbers; third-party dependencies emit no debug information.
Tests inherit the development settings, and benchmarks inherit the release
settings. These profiles are configured in the workspace's `.cargo/config.toml`
so they take precedence over user-wide Cargo profile settings.

## CI build caches and timings

The Cargo workflow (`.github/workflows/ci.yml`) runs on stable Rust for every
push to `main` and every PR. It runs the all-feature suite, doctests and a
default-feature build on both `ubuntu-24.04` and `ubuntu-24.04-arm`; function
and build cache keys include runner architecture. Optional engines are built
and exercised independently. Its jobs run in parallel:

- `test`: four shards of the full all-features suite under
  [cargo-nextest](https://nexte.st), split with `--partition hash:K/4` so a
  test stays in the same shard from run to run. The Docker proxy tests are
  excluded; they run in `proxy_tests`. Each shard uploads its JUnit report
  (`junit-shard-K`) with per-test durations.
- `doctests`: builds default features once, then runs the doctests, which
  nextest does not run.
- `lint`: format, clippy and book links.
- `proxy_tests`: the [reverse-proxy tests](proxy-tests.md).

Optional-engine jobs use `runtime-checks` for both module tests and tutorial
examples, reusing the compiler, embedded runtime and integration test binary.
Cargo also builds the CLI for the integration target, so the sales-report
application uses that binary without a separate compiler build.
`just ci dataframe linalg` follows the same rule locally. The
`doctests` job still verifies a default-feature compiler build separately.

The shards build with the `ci` Cargo profile, defined in `.cargo/config.toml`:
the `dev` profile with the `plenty` crate at `opt-level = 1` and dependencies
at `opt-level = 2`, keeping debug assertions and overflow checks. Nearly every
test compiles a Plenty program, so suite time is mostly the compiler's own
run time, and an optimized compiler shortens it by about a third. Building
takes longer, so the local default stays `dev`, which rebuilds faster after an
edit. For a local full-suite run, `cargo nextest run --locked --workspace
--all-features --cargo-profile ci` uses CI's build; its artifacts live in
`target/ci/`. The `doctests` and `proxy_tests` jobs run few tests per build
and stay on `dev`. The nightly workflow also stays on `dev`, the profile
`cargo test` uses locally, so the full suite still runs weekly with an
unoptimized compiler.

Each shard compiles the tests itself. A shared build job with
`cargo nextest archive` would compile once, but the shards would then wait for
that job's setup, compilation and upload, so it saves runner time, not wall
time.

Jobs that run tests install mold from Ubuntu's package and fail unless `cc`
links with `/usr/libexec/mold/ld` (`scripts/ci_check_mold.sh`); see
[Running tests](workflow.md#running-tests) for why. A weekly workflow,
`.github/workflows/nightly.yml`, runs the full all-features suite with
`cargo test` on nightly Rust, also on demand through `workflow_dispatch`.

### Native OS qualification

The experimental macOS ARM64 and Windows x86-64 checks in
`.github/workflows/portability.yml` build the compiler and dataframe engine
concurrently. Compiler jobs retain stable-Rust boundary checks, build and unit
tests, Darwin stack-probe integration tests, and the pinned-release compiler
build and unit tests. Engine jobs use the pinned release Rust and run the
baseline engine build, formatting check, tests and clippy.

A dependent packaging job on each native OS downloads both products from the
same workflow run. Tar artifacts retain executable modes and macOS signatures;
packaging still audits the executable and engine baselines, installs notices,
and runs the complete archive smoke checks, including relocation. Packaging
requires both build matrices to succeed. No new OS is admitted by these checks.

Stable qualification dependencies live in `target/qualification/`, pinned
release compiler dependencies in `target/native-release/`, and engine
dependencies in `plenty-dataframe/target/`. Separate cache actions keep cleanup
from removing another workspace's dependencies. Keys distinguish each role,
target and Rust/compiler environment; release target policy and engine build
policy also participate. Only successful `main` jobs save caches; PRs restore
them. Workspace code still rebuilds. The release CPU/runtime flags and macOS
deployment minimum remain unchanged, and custom `PLENTY_RUNTIME_RUSTFLAGS`
still bypass embedded-runtime vendor/archive reuse.

Before this split, successful runs 42350500941 and 42344839963 took 31–41
minutes on Windows and 29–34 minutes on macOS. In the first run, engine
compilation alone took 13m31s and 10m50s respectively. Cold runs now overlap
that work with compiler qualification; warm runs can reuse third-party Rust
dependencies. Hosted cache transfer, hit rates and resulting elapsed savings
must still be measured; the first run for each key remains cold.

### Dependency cache

Third-party Rust dependencies are cached separately for each job. The key
includes the compiler, lockfile, manifests, Cargo configuration and compiler
environment. Only successful `main` runs save caches, and only the first shard
saves the shards' shared dependency cache; PRs restore the base branch's caches
without filling storage with per-PR copies. Workspace artifacts and incremental
state are not retained. The [workspace profiles](#rust-build-profiles) also keep compilation outputs small. The
nightly workflow keeps no cache: each nightly update invalidates compiled
dependencies.

Optional-engine jobs keep compiler dependencies in `target/` and engine
dependencies in `plenty-dataframe/target/` or `plenty-linalg/target/`. These
cache paths must not overlap: the cache action cleans each target with its
workspace's dependency list, so cleaning a parent target would discard nested
engine-only dependencies. The engine build receives the matching explicit
`--target-dir`; its installed library still goes to `target/engine/`. The
`baseline-isolated-v2` engine cache key starts a new cache population; the
first run is cold, and hosted timings must confirm reuse on later runs.

### Embedded-runtime Rust dependencies

The test shards also retain the 27 vendored Rust rlibs that `build.rs`
compiles for both embedded runtimes. They are independent of runtime source,
Cargo profile and allocation-check mode. A separate cache outside `target/`
survives workspace-artifact cleanup. Only a successful `main` run's first
shard saves it; both architectures have separate entries.

The build script validates a SHA-256 key over the vendored source tree,
build logic, lockfile, target, Rust compiler version and target standard-library
contents. It checks each restored rlib's content before reuse and publishes
complete entries atomically. Native compilation and finished runtime archives
use the separate boundaries described under [native and runtime archive reuse](#native-and-runtime-archive-reuse).
Custom `PLENTY_RUNTIME_RUSTFLAGS` bypass reuse because flags may name extra
files that the key cannot infer. Unreadable or symlinked key inputs also bypass
reuse; they do not prevent an otherwise valid compiler build.

Locally, entries live under the Cargo profile's
`plenty-runtime-vendor-cache/` directory and survive runtime-source and
feature-only rebuilds. `PLENTY_RUNTIME_VENDOR_CACHE=/absolute/path` shares
entries across profiles or isolated targets; `PLENTY_RUNTIME_VENDOR_CACHE=off`
disables reuse. Delete the directory to reclaim entries after a dependency or
toolchain change. Cache status and dependency-stage duration appear in verbose
Cargo build output; saved Cargo timings still report the whole build script.

A local 2026-11-06 probe reduced this dependency stage from 18.95 seconds to
0.46 seconds, removing all 27 vendored rustc invocations on a hit. Whole
build-script samples were 82.16 seconds before caching, 82.23 seconds with a
cold cache and 108.21 seconds with a warm cache: native/runtime work varied on
the shared machine. Those samples establish a dependency-stage saving, not an
end-to-end speedup. Hosted restore/save overhead and CI savings remain to be
measured. The native and runtime archive stages use separate caches.

### Native and runtime archive reuse

Linux test shards install ccache and restore its object cache outside the Cargo
workspace. Preprocessor mode checks actual source, flags and SDK/system-header
contents on every invocation, including newly appearing headers. Compiler
identity uses its contents, with discovered GCC backend, assembler and specs
contents fingerprinted once per check into a ccache namespace. No global ccache
configuration or sloppiness is loaded. Native archives are assembled each time with the selected archive
tool, so its flags and identity do not need a separate archive cache key.
[The ccache manual](https://ccache.dev/manual/latest.html#_the_preprocessor_mode)
describes this boundary. Assembly that ccache cannot cache follows the ordinary
compiler path.

`build.rs` then keys each application/library runtime archive and its link
metadata on the finished native archives and rlibs, runtime sources and manifest,
recipe, target, Rust toolchain/standard-library contents and allocation-check
mode. This avoids guessing native header inputs at the runtime archive boundary.
Restored artifacts are content-validated; per-key locks and atomic publication
protect concurrent builds. Custom `PLENTY_RUNTIME_RUSTFLAGS`, unsupported key
inputs and `PLENTY_RUNTIME_ARCHIVE_CACHE=off` bypass archive reuse.

Local archive entries live in the profile's `plenty-runtime-archive-cache/`;
`PLENTY_RUNTIME_ARCHIVE_CACHE=/absolute/path` can share them across isolated
targets. Delete the cache to reclaim entries. Hosted shards restore these
archives separately from workspace cleanup. Only a successful `main` run's
first shard saves native and runtime entries, independently for each architecture.
The compiler build's verbose output reports SQLite, Brotli, ring and runtime
archive durations; ccache statistics are also printed by hosted test shards.

On Linux, `just test` and `just ci` automatically use an installed ccache, with
objects under `target/native-ccache/` (or `CCACHE_DIR`). Without ccache (or with an older release lacking namespaces) they use
the ordinary compiler. `PLENTY_NATIVE_CACHE=off` disables this automatic setup.
Explicit compiler selections or wrappers, custom C flags or other ccache settings preserve
the caller's environment. Only `HOST_CC` is selected automatically; cross-target
compiler discovery remains with cc-rs. Direct Cargo commands do not configure
ccache automatically, but do use the runtime archive cache.

Measurements and invalidation probes for issue #832 are recorded in the
[completed-work entry](../completed-work/language-and-tooling.md#native-and-runtime-archive-reuse-issue-832).
Hosted cache transfer and overall CI savings still need measurement.

### Compiled-function cache

Each shard also keeps the function entries of the
[compiled-function cache](../design/toolchain/02-architecture/cache.md),
`target/plenty-cache/fn`, in its own GitHub Actions cache. The dependency
cache deletes that directory, so it has its own restore and save steps. Their
keys depend on the compiler and Cranelift versions and the compiled IR, so
entries stay valid across commits. Program entries, `target/plenty-cache/program`,
key on the compiler binary's modification time, never hit in a new CI build,
and are not kept.

The cache needs no maintenance. A cache hit only reads its entry, so
`scripts/ci_fn_cache.sh mark` gives every restored entry an old access time,
and the next read updates it. After a successful `main` run,
`scripts/ci_fn_cache.sh prune` deletes the entries the run did not read or
write. The saved cache is therefore the shard's working set for that commit:
it grows only with the suite, and entries for replaced code generation leave
it at the next save. The cache key ends with a digest of the remaining entry
names, so an unchanged working set is not saved again. A save deletes the
cache it replaces, so storage holds about one cache per shard. If reads do not
update access times on the runner, `mark` warns and nothing is pruned or
saved. GitHub removes a cache unused for seven days; the next run is then
cold, as is the first run after the shard count changes.

### Timings

The Cargo workflow ends with a **Check timing report** job, also after failed
checks. Its job summary shows jobs ranked by duration and a table of step names
aggregated across shards and architectures, with counts, seconds, status and
share of measured step time. The `check-timings` artifact retains the Markdown,
normalized JSON and raw Actions timestamps for 30 days. The reporting job itself
is excluded. Cancellation can prevent a report from being produced.

The same small helper can report on an existing run of any workflow. It reads
job/step timestamps through an authenticated `gh` CLI, including all pages and
only the latest run attempt; it does not download logs or rerun checks:

```sh
python3 -B scripts/check_timings.py ci --repo OWNER/REPO --run RUN_ID
python3 -B scripts/check_timings.py report target/check-timings/ci-CURRENT/timings.json --baseline target/check-timings/ci-EARLIER/timings.json
```

Use the output directories printed by the commands. Comparisons show the
absolute delta for every step name; a changed number of steps is marked rather
than treated as a slowdown. Job and step status remains visible when checks
fail. A report collected during a run is partial; unfinished jobs have no
completed duration. Raw timestamps remain in `jobs.json` and run metadata in
`run.json` for further inspection.

Elapsed span (first job start to last job finish) and summed runner-step time
are separate: jobs run concurrently, and runner time cannot be summed into
workflow latency. Initial queue time is outside the span. Step timestamps have
second resolution and do not attribute scheduling gaps between steps. Compare
the same workflow, architectures, shard counts and cache conditions; cache
restore/save and setup/post-action costs are included as their own steps.
These reports locate growing process steps; existing JUnit and Cargo artifacts
provide more detail when needed.

Test compilation is a separate step from execution. The `cargo-timings-*`
artifacts retain Cargo's HTML compilation reports for seven days, including
reports available after a failed run: `cargo-timings-tests` from the first
shard, `cargo-timings-build` from the default-feature build,
`cargo-timings-proxy` and `cargo-timings-nightly`.

Documentation caches only the pinned mdBook executable, keyed by its version,
Rust version, runner OS, architecture and Ubuntu image. Every run checks its
version before building. Cache hits skip both Rust setup and `cargo install`.
Superseded PR runs are cancelled; main documentation deployments are not
interrupted. Release packaging keeps its isolated, pinned build environment.

The first run for a compiler or cache key remains cold. Compare the restore,
compile, test and cache-save step durations on hosted runs before attributing a
speedup; cached dependencies do not eliminate compiling Plenty itself.

## GitHub source highlighting

Plenty is its own language. Until GitHub supports it through Linguist, the root
`.gitattributes` selects Python highlighting for every `.plenty` file, including
nested modules. This is approximate coloring: Plenty-specific syntax can be
miscolored, and a file viewer may label the file as Python. The
`linguist-detectable=false` attribute excludes these files from repository
language statistics so they do not count as Python; genuine Python files keep
their normal classification. The files remain visible in diffs.

This override applies only to repositories containing the rule. It does not
provide editor integration or guarantee GitHub code navigation or search
classification. Keep Markdown's `plenty` fences and the book's custom highlighter;
they are independent of GitHub's source viewer. Once native Plenty support
reaches GitHub, remove both temporary attributes and recheck source highlighting
and language statistics.
