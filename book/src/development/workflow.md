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

The Cargo workflow caches third-party Rust dependencies separately for each
job and toolchain. The key includes the compiler, lockfile, manifests, Cargo
configuration and compiler environment. Only successful `main` runs save
caches; PRs restore the base branch's caches without filling storage with
per-PR copies. Workspace artifacts and incremental state are not retained.
The line-table-only workspace profiles and disabled dependency debug information
above also keep compilation outputs small.

Stable checks default-feature linking once. Stable and nightly both compile
and run the full all-feature test suite; clippy and Docker proxy checks keep
their existing coverage. Test compilation is a separate step from execution,
and the `cargo-timings-*` artifacts retain Cargo's HTML compilation reports for
seven days, including reports available after a failed run. Doctests still run
in the test step, so that step can include their compilation time.

Documentation caches only the pinned mdBook executable, keyed by its version,
Rust version, runner OS, architecture and Ubuntu image. Every run checks its
version before building. Cache hits skip both Rust setup and `cargo install`.
Superseded PR runs are cancelled; main documentation deployments are not
interrupted. Release packaging keeps its isolated, pinned build environment.

The first run for a compiler or cache key remains cold. In particular, rolling
nightly updates invalidate compiled dependencies frequently. Compare the restore,
compile, test and cache-save step durations on hosted runs before attributing a
speedup; cached dependencies do not eliminate compiling Plenty itself.

## Running tests

`cargo test --locked --workspace --all-features` runs the full suite, as CI
does. When several workers share a machine, add `-j 3 -- --test-threads=3`.

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
linker. The setting does not change the compiler's default `cc` driver or the
Rust linker; `COMPILER_PATH= cargo test ...` links with the system default.
Tests must not depend on one linker's output, such as the exact symbol table.

The same file points `PLENTY_CACHE_DIR` at `target/plenty-cache`, so every
test run of a worktree reuses the
[compiled-function cache](../design/toolchain/02-architecture/cache.md) from
the runs before it and never writes to `~/.cache`. A fresh cache must not
change any result: `PLENTY_CACHE=off cargo test ...` runs without it, and a
test that exercises the cache uses a directory of its own.

[cargo-nextest](https://nexte.st) is optional: `cargo nextest run --workspace
--all-features` runs each test in its own process. It does not run doctests.

## Runnable documentation

`tests/test_tutorial.rs` reads tutorial and `design/examples/` pages in
`SUMMARY.md` order. A `plenty` fence has an `output` fence and runs both through
compile-and-run and an explicitly compiled binary. A `plenty-error` fence has an
`error` substring and must fail without executing effects. A `plenty-file`
companion module remains on the same page as its example. Every book page must
appear in `SUMMARY.md`. The examples run as 16 tests in parallel, each taking
every 16th example in reading order; a failure names the example's page and
line.

`book/theme/plenty-highlight.js` registers the highlight.js grammars for these
fences: Plenty source for `plenty`, `plenty-error` and `plenty-file`, compiler
diagnostics with highlighted `` `code` `` for `error`, and plain text for
`output`. `book/theme/plenty-highlight.css` colours the classes the bundled
themes leave unstyled. Its keyword, literal, type and built-in lists follow the
compiler; `tests/test_book_highlighting.rs` fails when a reserved word or
lower-case prelude name is missing from them, or when they name one the compiler
no longer defines.

Proposal fences are illustrative and are not executed by the tutorial suite.
Use `text` fences for proposed syntax; check implemented behavior with a complete
program or an executable lesson.

After documentation moves, run:

```sh
cargo test --locked --test integration test_tutorial::
mdbook build book
python3 -B scripts/check_links.py book/src
```

The link checker uses Python's standard library. It checks local file targets in
inline links, images, and reference definitions, ignoring code and comments.
It does not check remote URLs, HTML links, or heading anchors. To check only a
changed area, pass a directory or Markdown file relative to `book/src`, for
example `python3 -B scripts/check_links.py book/src proposals`.

Check the workflow helpers with
`python3 -B -m unittest discover -s scripts -p 'test_*.py'`.

## Documentation site

[The public book](https://cjrh.info/plenty/) defaults to development until the
first published release is admitted in `book/releases.json`. After that, the
root serves the selected stable release. [Development documentation](https://cjrh.info/plenty/dev/)
always follows `main`; admitted releases live under `/plenty/VERSION/`.
Every page identifies its version and offers a version menu. Matching chapters
are preserved when switching versions; missing chapters fall back to that
version's homepage. The links work without JavaScript; JavaScript also preserves
the current query and heading fragment when the chapter exists.

`.github/workflows/docs.yml` builds every push to `main`, PR targeting `main`
(including forks), and manual dispatch. Only a main push or main dispatch can
deploy. It does not publish packages or push branches. Release admission and
retention are described in [versioned documentation](documentation-versions.md).

Use the same pinned tools as the workflow:

```sh
rustup toolchain install 1.99.0 --profile minimal
cargo +1.99.0 install mdbook --version 0.5.4 --locked
python3 -B scripts/check_links.py book/src
python3 -B scripts/build_docs_site.py
mdbook serve book --open
```

For only the current checkout, `python3 -B scripts/build_book.py` writes
`book/book`. The complete site helper writes `book/site`. Both show Development
and its source commit, explicitly marking local changes. Each version's
`docs-version.json` records the full revision and Cargo package version;
the package version alone does not identify a development build. Labels are
static HTML. Raw `mdbook serve` previews content without deployment metadata.
To preview the complete site, mount `book/site` at `/plenty/` on a static HTTP
server; configure unknown paths to serve its root `404.html` as GitHub does.

The build needs no Plenty compiler. Do not use `mdbook test` for Plenty fences;
the tutorial integration tests above execute those examples. The output checker
verifies rendered local links stay inside the artifact and reach existing files,
and checks search, print, 404 and custom highlighting assets. Source checking
alone cannot catch a malformed `SUMMARY.md` that leaves a chapter unrendered.
Neither checker validates external URLs or heading anchors.

The Pages artifact is only `book/site`, which remains ignored. It contains all
admitted versions together, so development updates preserve older books. Each
build receives its own `site-url` prefix for nested 404 assets. Check navigation,
anchors, search, print, version switching and Plenty highlighting under each
prefix; repeat those checks on the public site after deployment.

### First publication and recovery

After review, merge and authorization to publish, an administrator must:

1. Check account/repository visibility and organization policies allow Pages
   and the pinned actions. Choose **Settings > Pages > Build and deployment >
   Source: GitHub Actions**. The workflow does not enable Pages automatically.
2. Set the `github-pages` environment's deployment branch policy to `main`
   only, preserving any required reviewers.
3. Run **Documentation** from current `main`, approve its environment if
   required, and inspect the deployed URL and source commit in the run summary.

Build jobs have only `contents: read`; PRs do not configure/upload Pages or use
the deployment environment. The deploy job has `pages: write`, `id-token: write`
and `contents: read` for a main-tip API check. It uses the built-in token, with
no PAT. Actions are pinned by SHA; the upload action also pins its nested action.

Deployment jobs share one concurrency group and do not cancel a running
deployment. After acquiring that slot and any environment approval, a run checks
the live main SHA and skips if superseded. Queue ordering is not guaranteed:
rerun current main if its pending deployment was replaced by an older run that
then skipped. A commit arriving after the check can briefly leave the previous
revision live until the next successful run; serialization prevents an older
deployment finishing after a newer deployment. API failures fail closed.

Failed checks or deployments leave the last successful site available. Rerun
current main for transient failures; fix or revert bad content through review
and deploy the resulting main commit. Do not roll back by dispatching an old
branch or deleting the site. Tool/action upgrades require another complete book
build and theme/navigation check. See the official
[Pages workflow guide](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)
and [mdBook HTML configuration](https://rust-lang.github.io/mdBook/format/configuration/renderers.html).

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

## Reverse-proxy tests

`tests/test_std_http_server_proxy.rs` runs `std.http.server` behind nginx and
Caddy (see [reverse proxies](../design/stdlib/http-qualification.md#reverse-proxies)).
They need Docker and curl, so they run only when asked:

```sh
PLENTY_DOCKER_TESTS=1 cargo test --locked --test integration test_std_http_server_proxy::
```

Without the variable each test prints that it is skipped and passes, so the
ordinary suite needs no Docker and does not slow down. With it, a missing
Docker daemon or curl fails the tests. The first run pulls the two pinned images
(about 70 MB each). Each test starts its own containers with `--network host`
on free loopback ports, names them `plenty-proxy-test-<pid>-<n>` with the label
`plenty-test=proxy`, and removes them when it ends, also on failure; nothing
else is stopped or pruned. A test process killed outright leaves its
containers running; list them with
`docker ps --filter label=plenty-test=proxy`.

CI runs them in the `proxy_tests` job of `.github/workflows/ci.yml` on
GitHub's `ubuntu-latest`, which has Docker and curl. The job pulls
`nginx:1.30.6-alpine3.24` and `caddy:2.11.7-alpine` in an earlier step, so
pulls are not timed with the tests; when the tests change an image tag,
change the job's pull step to match.

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
