# Allocator fork filetests

Plenty's locked regalloc2 fork can allocate differently from the registry
regalloc2 used by Cranelift's own workspace. Passing ordinary Cranelift
filetests therefore does not establish compatibility with Plenty's allocator.
Issue #979 adds a separate integration check, keeping the Cranelift fork's
registry dependency and upstream expected output intact. This avoids coupling
its manifest to another fork or blessing allocator-dependent output as though
it were an upstream Cranelift change.

## Run the check

From a Plenty worktree:

```sh
just fork-filetests
# Explicit local object repositories or report location:
python3 -B scripts/check_fork_filetests.py --wasmtime /path/to/wasmtime \
  --regalloc2 /path/to/regalloc2 --output /tmp/fork-filetests
```

Defaults find sibling forks beside the primary Plenty checkout, including
when invoked from a task worktree in `/tmp`. Both local repositories must
contain the revisions in this worktree's `Cargo.lock`. The command reads local
Git objects only: it does not fetch, push, change a branch or use dirty fork
files. It exports both committed revisions into a fresh report directory,
then patches only the exported Cranelift build to the exported regalloc2 path.

Cargo runs offline. Rust/C build tools and cached dependencies for
`cranelift-tools` must already be available. Missing crates are a setup failure,
not a passing check. The tool clears inherited Rust flags, compiler wrappers
and `CRANELIFT_TEST_BLESS`; it selects the x86 backend and disables optional
`cranelift-tools` default features. Build outputs are isolated per report.
The check is a local development command, not part of routine CI, which does
not have these sibling object repositories.

## Read the report

Each `target/fork-filetests/run-*` directory retains:

- `summary.json`: exact fork pins, commands, phase exit statuses and result;
- `registry-*` and `plenty-*` logs for dependency resolution, build and x64 tests;
- the resolved lockfile for each allocator variant;
- exported source and build outputs for reproducing a failing case.

Dependency trees are checked to ensure the baseline uses registry regalloc2
and the patched build actually uses the exported allocator. The runner executes
all `cranelift/filetests/filetests/isa/x64` tests in each variant, including
precise assembly-output checks. Logs retain their complete mismatch diagnostics.

Exit zero means both variants passed. Exit 1 means the baseline passed but
the Plenty-allocator run had differences or other failures. Exit 2 means setup,
build or the registry baseline failed; diagnose that before attributing an
output difference to the allocator. No mismatch is automatically accepted or
filtered. The check never sets `CRANELIFT_TEST_BLESS`, even in its isolated
exports, and never edits expected output in either fork checkout.

Issue #979 originally observed 44 differing x64 files. That count is historical,
not a permanent allowlist. Review each current mismatch for an allocator-only
instruction change, an invalid allocation, or an unexpected semantic/backend
failure. Keep the report with the fork PR's evidence. Ordinary fork tests and
Plenty's semantic suite remain required; a textual mismatch alone neither
proves a compiler defect nor establishes equivalent behavior.

For a proposed fork change, first run this command at Plenty's current pins.
Then use a temporary lockfile change on the task branch to select the candidate
commits and rerun. Compare both reports; do not change locked dependencies merely
to silence expected-output failures. Local dirty edits are deliberately excluded:
commit a candidate to an isolated local fork branch before testing it. Publication
still follows the [fork procedure](workflow.md#cranelift-and-regalloc2-forks)
and requires the user's authorization.
