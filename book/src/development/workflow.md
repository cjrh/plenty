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

## Runnable documentation

`tests/test_tutorial.rs` reads tutorial and `design/examples/` pages in
`SUMMARY.md` order. A `plenty` fence has an `output` fence and runs both through
compile-and-run and an explicitly compiled binary. A `plenty-error` fence has an
`error` substring and must fail without executing effects. A `plenty-file`
companion module remains on the same page as its example. Every book page must
appear in `SUMMARY.md`.

Proposal fences are illustrative and are not executed by the tutorial suite.
Use `text` fences for proposed syntax; check implemented behavior with a complete
program or an executable lesson.

After documentation moves, run:

```sh
cargo test --locked --test test_tutorial
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
PLENTY_DOCKER_TESTS=1 cargo test --locked --test test_std_http_server_proxy
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
