# Development workflow

The [backlog](../backlog.md) is the sole list of future work and priorities.
Issues, claims, and local PR reviews live in the shared `issues.db`; the repository's
`AGENTS.md` and `CLAUDE.md` describe the worktree and review workflow.

Keep implementation, reference contracts, and executable lessons aligned in the
same change. Reference pages describe implemented rules and current limits;
proposals preserve rationale and alternatives. Update a backlog entry when its
scope changes or completes.

Native tests should check observable output, diagnostics, evaluation order,
mutation, ownership, borrowing, and cleanup. Allocating operations need failure
and partial-state cleanup checks. Use runtime allocation checks, sanitizers, or
Miri where they exercise the changed boundary.

## Build a complete local installation

Install [just](https://just.systems/man/en/packages.html), a stable Rust toolchain
with its `rust-docs` component, Python 3, and the native C compiler/linker and
binutils (`readelf`, `nm`). From the checkout, run:

```sh
just                       # same as just build
just compiler              # compiler only
just engine dataframe      # one engine and its redistribution notices
just engine linalg
just test                  # all-features workspace suite, with a timing report
just docs                  # all three published-site checks
just ci                    # every CI check this Linux host can run
just release-check         # just ci plus the Docker release build and smoke tests
just --list
```

The default builds release `plenty` and `plentyc`, then both optional engines,
sequentially. The first engine builds are substantial; later builds reuse Cargo
artifacts. `target/release/` contains both executables, both shared libraries,
and `dataframe-notices/` and `linalg-notices/` with checksummed metadata and
licenses for `--dist`. Missing Rust license files can be installed with
`rustup component add rust-docs`. Cargo may need network access for uncached
registry and pinned Git dependencies.

Add the checkout's absolute `target/release` directory to PATH. For Fish:

```fish
fish_add_path /path/to/plenty/target/release
```

Rebuilding updates this installation; moving the checkout or cleaning its
artifacts removes it. The compiler recipe explicitly selects `target/`, even
if `CARGO_TARGET_DIR` is set. Engine caches stay under
`target/dataframe-engine/` and `target/linalg-engine/`. Ordinary
`cargo build --release --locked` remains compiler-only. Existing Python engine
build commands remain available for custom output paths and job counts.

These are **local host builds**, not portable release archives. Local packaging
retains architecture, SONAME, export, dependency and RPATH checks, but records
the engine's actual required glibc symbol version rather than enforcing 2.28.
It also records `build_kind: local` and whether the checkout is dirty (excluding
the shared issues register); this is provenance information, not an attestation.
The compiler and generated applications can require newer system libraries too.
The explicit `--local-install` packaging mode does not produce release archives
or enforce the portable GLIBC/GCC symbol baseline; the release workflow never
selects it. [Portable releases](releasing.md) keep the strict baseline audit.

Do not run builds against the same installation concurrently. Recipes stop on
the first failure; the complete installation is not a transaction. Notices are
staged before replacement, and `--dist` rejects a library whose checksum no
longer matches its notices after an interrupted update. Re-run the recipe to
repair that installation. Existing notices directories are generated output and
are replaced, not a location for application-owned files.

## Check timing reports

`just test` runs the full all-features workspace suite, including doctests,
with compilation timed separately from test execution. It saves Markdown and
JSON reports under a unique `target/check-timings/suite-*/` directory, and
retains Cargo's compilation HTML under `target/cargo-timings/`. Output stays
on the terminal and the command returns failure if compilation or tests fail.
The execution phase includes Cargo's short freshness check and doctest builds;
this is command-level attribution, not a compiler or per-test profiler.

```sh
python3 -B scripts/check_timings.py suite --profile ci
python3 -B scripts/check_timings.py report target/check-timings/suite-CURRENT/timings.json --baseline target/check-timings/suite-EARLIER/timings.json
```

On Linux, both check commands automatically use an installed ccache for native
compilation, while preserving explicit compiler selections. Its objects live in
`target/native-ccache/`; `PLENTY_NATIVE_CACHE=off` disables automatic setup.
The [native and runtime cache guidance](ci-workflow.md#native-and-runtime-archive-reuse)
explains invalidation, other settings and the separate runtime archive cache.

`just ci` also saves each invocation's step logs, `steps.tsv`, `timings.json`
and `timings.md` under `target/local-ci/run-*/`, including failed steps. It
continues to later checks after a failure. These timings use Bash's integer
seconds; the suite wrapper uses a monotonic clock. Both add negligible work
at command boundaries and retain separate runs for comparison.

Direct `cargo test` remains available but does not save this report. Reports
record the revision and host; note profile, cache warmth, competing builds,
Cargo lock waits and environment overrides when comparing runs. JSON command
rows record the suite's actual Cargo arguments. Measurements observe ordinary
work; they do not reserve the host or claim controlled benchmark results.

For CI job/step reports and historical comparisons, see
[CI timings](ci-workflow.md#timings).

## Dependency versions

The compiler executable and runtime share a tracked workspace `Cargo.lock`.
Fresh checkouts and worktrees use its dependency versions; CI builds and tests
with `--locked` so a stale or missing lockfile fails instead of resolving a new
dependency graph. Use `--locked` for local validation too.

Update dependencies deliberately with `cargo update`, and review the resulting
lockfile changes with the code that needs them. Changes to Cranelift or regalloc2
require rerunning object-identity checks and affected compiler performance
comparisons. Record the dependency versions with benchmark results so results
from different dependency graphs are not compared as if only Plenty changed.

### Cranelift and regalloc2 forks

Plenty builds Cranelift and regalloc2 from its own forks, so a code generation
defect whose cause is in either can be fixed there:

| Crate | Repository | Branch | Base |
|---|---|---|---|
| `cranelift-*` | <https://github.com/cjrh/wasmtime> (Cranelift is in `cranelift/`) | `fork` | Wasmtime 50.0.0, Cranelift 0.137.0, with upstream #14717 (issue #934), loads merged into a block's branch and into the left operand of an x86-64 integer compare (issue #935), and optional spill-reload folding and forwarding after register allocation (settings `regalloc_fold_reloads` and `regalloc_forward_spills`, issue #930) |
| `regalloc2` | <https://github.com/cjrh/regalloc2> | `fork` | 0.15.2 and later upstream commits, with the loop-exit block fix (`docs/cold-exit-block-splits.md` in the fork), split bundles moved into their spill bundle's register (`docs/spill-bundle-registers.md`, issue #939), and a spilled value's register extended over its loop when one is free (`docs/loop-extension.md`, issue #939) |

`Cargo.toml` names the `cranelift-*` crates as Git dependencies. Cranelift
itself names the crates.io `regalloc2`, so `[patch.crates-io]` replaces it; the
fork's version must satisfy Cranelift's requirement. `Cargo.lock` records the
exact revision of each fork, and `--locked` builds use it. Both projects are
licensed Apache-2.0 WITH LLVM-exception; keep their license files and headers,
and state changes in commit messages.

To change a fork:

1. Work in the fork's checkout beside the primary worktree (`../wasmtime`,
   `../regalloc2`), on a branch from `fork` or in a worktree of it.
2. Build Plenty against the checkout with an uncommitted override in the task
   worktree's `Cargo.toml`, without `--locked`. Set `PLENTY_CACHE=off` for
   every compiler run: the compiled-function cache key holds the locked fork
   revisions, and a path override has none, so cached code from an earlier
   edit would be reused:

   ```toml
   [patch.crates-io]
   regalloc2 = { path = "../regalloc2" }

   [patch."https://github.com/cjrh/wasmtime"]
   cranelift-codegen = { path = "../wasmtime/cranelift/codegen" }
   cranelift-frontend = { path = "../wasmtime/cranelift/frontend" }
   cranelift-module = { path = "../wasmtime/cranelift/module" }
   cranelift-object = { path = "../wasmtime/cranelift/object" }
   cranelift-native = { path = "../wasmtime/cranelift/native" }
   ```

3. Run the fork's own tests (`cargo test` in regalloc2, which includes its
   fuzz targets' smoke runs; the Cranelift filetests for a Cranelift change),
   then Plenty's suite and the [emitted-code](emitted-code.md) measurements
   against the override.
4. Commit to the fork's `fork` branch and push it. Pushing publishes; an agent
   needs the user's authorization.
5. In the Plenty PR, remove the override, run
   `cargo update -p regalloc2` or `cargo update -p cranelift-codegen`, and
   commit the lockfile with the measurements. Update
   `experiments/kernels/harness/Cargo.lock` the same way so hand-written CLIF
   is compiled by the same backend.

Keep each fork commit small and self-contained, with the reproduction in its
message, so it can be offered upstream. To take an upstream release, rebase
`fork` onto the release tag, resolve conflicts with the fork's commits, and
treat the result as a dependency update. `scripts/aarch64-probe` and the older
experiments keep their own crates.io pins.

<span id="rust-build-profiles"></span><span id="ci-build-caches-and-timings"></span><span id="dependency-cache"></span><span id="compiled-function-cache"></span><span id="timings"></span><span id="github-source-highlighting"></span>

## Build profiles and CI

The workspace uses Rust edition 2024. Development and release builds disable
incremental compilation; workspace code keeps line tables for backtraces, while
dependencies have no debug information. See [CI and repository configuration](ci-workflow.md)
for the build profiles, jobs, caches, timings, and source highlighting.

For the local CI-profile full suite, run:

```sh
cargo nextest run --locked --workspace --all-features --cargo-profile ci
```

## Running tests

`cargo test --locked --workspace --all-features` runs the full suite: the
tests CI runs in shards under nextest, plus the doctests. When several workers
share a machine, add `-j 3 -- --test-threads=3`.

CI's `lint` job also gates `cargo fmt --all -- --check`,
`cargo clippy --locked --workspace --all-targets -- -D warnings`, and
`python3 -B scripts/check_links.py book/src` on stable Rust. Run them before
committing.

The integration tests in `tests/test_*.rs` are modules of one test binary,
`tests/integration.rs`, so all of them share one pool of test threads and the
binary is linked once. Select one file's tests by module, or one test by path:

```sh
cargo test --locked --test integration test_std_url::
cargo test --locked --test integration -- --exact test_std_url::rfc_3986_examples_split_into_components
```

Cargo builds separately for each set of `--workspace` and feature flags, so
add the full suite's flags to reuse its build. List a new test file in
`tests/integration.rs`; `every_test_file_is_listed` fails until it is. Editing
any test file recompiles the whole binary.

Most tests compile a Plenty program and link it with `cc`. The workspace's
`.cargo/config.toml` sets `COMPILER_PATH` for the processes Cargo starts, so
where mold is installed as `/usr/libexec/mold/ld`, `cc` links these programs
with mold, several times faster than GNU ld. Elsewhere `cc` uses its default
linker. CI installs mold for this. The setting does not change the compiler's default `cc` driver or the
Rust linker; `COMPILER_PATH= cargo test ...` links with the system default.
Tests must not depend on one linker's output, such as the exact symbol table.
The file also sets `PLENTY_LINKER_THREADS=1`, so each test's link runs mold
on one thread
([Native linking](../design/toolchain/01-building/linking.md)): many links
run at once, and mold's default thread per core made them compete for the
cores. `PLENTY_LINKER_THREADS= cargo test ...` restores mold's default.

The same file points `PLENTY_CACHE_DIR` at `target/plenty-cache`, so every
test run of a worktree reuses the
[compiled-function cache](../design/toolchain/02-architecture/cache.md) from
the runs before it and never writes to `~/.cache`. A fresh cache must not
change any result: `PLENTY_CACHE=off cargo test ...` runs without it, and a
test that exercises the cache uses a directory of its own.

[cargo-nextest](https://nexte.st) is optional locally; CI uses it.
`cargo nextest run --locked --workspace --all-features` runs each test in its
own process and does not run doctests (`cargo test --locked --workspace
--all-features --doc` does). Its `-j` sets test threads, not build jobs: when
sharing a machine, use `--build-jobs 3 --test-threads 3`. Because each test is
its own process, a `static` lock in a test file no longer keeps two tests
apart: tests that must not overlap belong to a test group in
`.config/nextest.toml`, as the timing tests of
`tests/test_compile_scaling.rs` do. `--profile ci` is CI's profile: it runs
every test after a failure and writes `target/nextest/ci/junit.xml`.

## Runnable documentation

`tests/test_tutorial.rs` reads tutorial and `design/` pages in `SUMMARY.md`
order. A `plenty` fence has an `output` fence and runs both through
compile-and-run and an explicitly compiled binary. A `plenty fragment` fence is
an explanatory excerpt, such as a signature shape, and is not run. The rendered
book labels it **Fragment · not executed**, outside the copyable source, while
retaining normal Plenty syntax colours. A `plenty-error` fence has an
`error` substring and must fail without executing effects. A `plenty-file`
companion module remains on the same page as its example. Every book page must
appear in `SUMMARY.md`. The examples run as 16 tests in parallel, each taking
every 16th example in reading order; a failure names the example's page and
line.

`book/theme/plenty-highlight.js` registers the highlight.js grammars for these
fences: Plenty source for `plenty`, `plenty fragment`, `plenty-error` and `plenty-file`, compiler
diagnostics with highlighted `` `code` `` for `error`, and plain text for
`output`. `book/theme/plenty-highlight.css` colours the classes the bundled
themes leave unstyled. Its keyword, literal, type and built-in lists follow the
compiler; `tests/test_book_highlighting.rs` fails when a reserved word or
lower-case prelude name is missing from them, or when they name one the compiler
no longer defines.

Proposal fences are illustrative and are not executed by the tutorial suite.
Use `text` fences for proposed syntax; check implemented behavior with a complete
program or an executable lesson.

Before handing off changes to book content, navigation, assets, configuration,
or docs build/check scripts, run the same checks as documentation CI:

```sh
python3 -B scripts/check_links.py book/src
python3 -B -m unittest discover -s scripts -p 'test_*.py'
python3 -B scripts/build_docs_site.py
```

Use the mdBook version pinned in `book/releases.json`; see [Documentation site](#documentation-site) for installation. Record the complete-site result in the PR.
If a check is blocked, state
which one and why rather than reporting documentation validation as complete.
The assembled-site check includes generated chapter and print links and version
navigation; a plain `mdbook build` is only a preview, not a substitute.

For moved or changed executable lessons, also run
`cargo test --locked --test integration test_tutorial::`. Prose-only changes
need the listed docs checks, not a compiler rebuild.

The link checker uses Python's standard library. It checks local file targets in
inline links, images, and reference definitions, ignoring code and comments.
It does not check remote URLs, HTML links, or heading anchors. To check only a
changed area, pass a directory or Markdown file relative to `book/src`, for
example `python3 -B scripts/check_links.py book/src proposals`.

<span id="first-publication-and-recovery"></span>

## Documentation site

Install the pinned tools and preview the book locally:

```sh
rustup toolchain install 1.99.0 --profile minimal
cargo +1.99.0 install mdbook --version 0.5.4 --locked
mdbook serve book --open
```

`python3 -B scripts/build_book.py` builds the checkout into `book/book`;
`python3 -B scripts/build_docs_site.py` builds and checks the complete versioned
site in `book/site`. Do not use `mdbook test` for Plenty
fences; the tutorial integration tests execute them.

See [documentation deployment](documentation-deployment.md) for site previews,
publication, permissions and recovery, and [documentation versions](documentation-versions.md)
for release admission and retention.

## Reverse-proxy tests

Run the nginx and Caddy integration tests when needed; they require Docker and curl:

```sh
PLENTY_DOCKER_TESTS=1 cargo test --locked --test integration test_std_http_server_proxy::
```

Without the variable, these tests report a skip and pass. With it, a missing
Docker daemon or curl fails them. See [reverse-proxy tests](proxy-tests.md) for
container cleanup and CI image maintenance.

## Quick reproductions

The compiler expects a complete program with a `main` entry point. Put probe
statements inside it; for allocation or printing, propagate failures:

```text
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    print(xs)?
    Ok(())
```

An infallible body can use `def main() -> ():`. The fragment wrapper in
`tests/support/mod.rs` is test fixture construction. For repeated probes, build
once with `cargo build --release`, then use `target/release/plenty FILE` from
that worktree; rebuild when compiler or runtime sources change.

Measure compilation cost when changing checking or
lowering; the [architecture guide](../design/toolchain/02-architecture/emission.md)
describes reproducible performance evidence. The opt-in
[compiler profiler](profiling.md) subdivides lowering, ownership checks and
native compilation. Use `compile_bench --workload check|scale|aot` to isolate a
workload, and `--no-warmup` when the report should contain only the requested
repetitions. Normal tests never start a profiling session automatically.
