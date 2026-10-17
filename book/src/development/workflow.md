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

Development and release builds disable incremental compilation. Workspace
sources (`plenty` and `plenty-runtime`) retain line tables for backtraces with
file names and line numbers; third-party dependencies emit no debug information.
Tests inherit the development settings, and benchmarks inherit the release
settings. These profiles are configured in the workspace's `.cargo/config.toml`
so they take precedence over user-wide Cargo profile settings.

## Running tests

`cargo test --locked --workspace --all-features` runs the full suite, as CI
does. When several workers share a machine, add `-j 3 -- --test-threads=3`.

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

For CI, use a Linux runner with Docker (GitHub's `ubuntu-latest` has Docker and
curl) and add a step that sets `PLENTY_DOCKER_TESTS=1` and runs the command
above. Pull `nginx:1.30.6-alpine3.24` and `caddy:2.11.7-alpine` in an earlier
step, or cache them, so pulls are not timed with the tests.

## Quick reproductions

The compiler expects a complete program with a `main` entry point. Put probe
statements inside it; for allocation or printing, propagate failures:

```text
def main() -> Result[(), Failure]:
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
describes reproducible performance evidence.
