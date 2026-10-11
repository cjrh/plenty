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
cargo test --test test_tutorial
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
