# Self-contained baseline harness specification

> **First implementation deliverable, not implemented here.** All paths and
> invocations below are proposed. Part of the [Cosmopolitan proposal](../cosmopolitan.md).
> This specifies reproduction, not a passing portability test or another work queue.

## Inputs and offline preparation

Check in `scripts/experiments/cosmopolitan/` with a README, a Python 3 driver,
a machine-readable input manifest and `fixtures/`. No invocation may depend on
the original `/tmp` directory, prebuilt compiler or unpublished scratch files.
The driver must use argument arrays, isolated output directories and explicit
compiler/tool paths, including paths with spaces. It must not modify the host's
loader registration, main checkout or global configuration.

Pin the baseline source to `2851ef3481f70dec580eb76d2651b86524d16dfb` and its
tracked Cargo.lock; record the actual source tree hash and any patch diff.
Pin Cosmopolitan 4.0.2 from
[the release archive](https://github.com/jart/cosmopolitan/releases/download/4.0.2/cosmocc-4.0.2.zip),
SHA-256 `85b8c37a406d862e656ad4ec14be9f6ce474c1b436b9615e91a55208aced3f44`
(bundled GCC 14.1.0). Retain the versioned upstream README, driver, signal/TLS
headers and fixup source with URLs, release/source identities and hashes.
See [official sources](evidence-and-gates.md#official-sources-and-source-credit-boundary).

The original installed rustc was `1.99.1 (58cf8f9c3 2026-10-13)` and host cc
was GCC `16.2.1 20260819`; neither identifies the toolchain that built the retained
Plenty binary. The harness implementation must resolve a complete Rust toolchain
pin (compiler, Cargo, target std and component hashes) and native host-tool inputs
in its manifest before running; it must not substitute a moving `stable` or
pretend to reconstruct the prebuilt binary. If those exact host versions cannot
be provisioned, record the deliberate replacement and treat output differences
as discrepancies to investigate. The retained compiler hash on the
[evidence page](evidence-and-gates.md) is comparison metadata, not a required input.

Proposed preparation invocation (not an existing command):

```text
python3 scripts/experiments/cosmopolitan/run.py prepare --manifest MANIFEST --inputs INPUTS
python3 scripts/experiments/cosmopolitan/run.py baseline --offline --inputs INPUTS --output OUTPUT
```

`prepare` is the only network-enabled phase: verify downloads, prepare the pinned
source checkout and Rust/Cargo dependency cache/vendor inputs and host tools,
and write a hash inventory. `baseline --offline` verifies that inventory, builds
with `cargo build --locked --offline --release --bin plenty` in isolated build
storage, and runs without network. Missing/corrupt inputs fail before building;
no implicit downloads or fallback to a cached prebuilt compiler. Repeat in two
clean output directories and compare input/object/archive hashes, recording any
nondeterminism rather than requiring unexplained executable byte identity.

## Complete original source fixture set

The following text is the required retained fixture content, not new runnable
book examples. Preserve filenames and line breaks for source-site diagnostics.
No upstream implementation code is copied by this proposal.

`fixtures/hello.c`:

```text
#include <stdio.h>
int main(void) { puts("cosmo C control"); return 0; }
```

`fixtures/empty.plenty`:

```text
def main() -> i32:
    0i32
```

`fixtures/print-alloc.plenty`:

```text
def main() -> Result[(), AllocError | IoError]:
    xs = [1, 2, 3]?
    print("cosmo Plenty", xs)?
    Ok(())
```

`fixtures/trap.plenty`:

```text
def divide(n: i64) -> i64:
    1 // n

def main() -> Result[(), IoError]:
    print(divide(0))?
    Ok(())
```

`fixtures/abi-control.c`:

```text
#include <stdio.h>
#include <stddef.h>
#include <signal.h>
int main(void) { printf("sigaction size=%zu flags=%zu mask=%zu restorer=%zu\n", sizeof(struct sigaction), offsetof(struct sigaction,sa_flags), offsetof(struct sigaction,sa_mask), offsetof(struct sigaction,sa_restorer)); return 0; }
```

## Command and artifact contract

Run the fixtures from an output-local copy so diagnostics use `trap.plenty`.
Record complete commands, environment/tool versions, elapsed times, stdout,
stderr and raw subprocess status separately for every case:

1. Build `hello.c` with `cosmocc/bin/cosmocc hello.c -o hello.com`; shell launch
   must print `cosmo C control` and exit 0. This C control can be dual-ISA; the
   Plenty tests use only `cosmocc/bin/x86_64-unknown-cosmo-cc`.
2. From the **fresh compiler**, extract `--emit-runtime runtime`. For each of
   empty, print-alloc and trap, use `--compile NAME.plenty --no-cache -o NAME.native`
   and `--emit-object NAME.plenty --no-cache -o NAME.o`. Native empty exits 0;
   print-alloc exits 0 with `cosmo Plenty [1, 2, 3]` plus newline; trap exits 1
   with `error: trap.plenty:2:5: division by zero` on stderr.
3. Test unchanged `--compile NAME.plenty --no-cache --no-strip --verbose
   --linker ABSOLUTE_COSMO_CC -o NAME.as-is.com` for empty and print-alloc.
   Baseline expects linker exit 1 mentioning missing `-lutil`.
4. Read extracted `runtime/link-args.txt` as one argument per line. Link each
   `NAME.o`, `runtime/libplenty_runtime.a`, all extracted arguments **except exact
   `-lutil` entries**, then `-o NAME.filtered.com`, using the x86-64 driver.
   Record original and filtered arrays; do not delete any other flags. Baseline
   expects all three links to succeed, empty/print output as native, and trap
   raw status -4 (`SIGILL`) with no diagnostic. Disable core dumps and run via
   `sh -c 'exec ./NAME.filtered.com'` with an eight-second timeout, preserving
   child status rather than relying on a shell's textual signal report.
5. Compile/run `abi-control.c` with native `cc` and the x86-64 Cosmo driver.
   Baseline output is respectively `sigaction size=152 flags=136 mask=8 restorer=144`
   and `sigaction size=32 flags=8 mask=24 restorer=16`, each with newline/status 0.
6. Attempt direct Python subprocess execution of hello.com and the two successful
   Plenty APEs. Record errno separately from child status. The spike observed
   `ENOEXEC` (8); loader/binfmt-equipped hosts may differ and must be identified,
   not reconfigured to manufacture the original result. Also record successful
   shell launch routes.

Retain sources, manifest, compiler/runtime/object/executable hashes, extracted
archives and link arguments, logs, disassembly/relocations and a JSON case report
with expected/actual outcomes, discrepancy classification and launch route.
Capture failures per case and continue independent cases; skip dependent cases
with explicit reasons. Unexpected failures, timeouts, missing evidence or baseline
discrepancies make the harness exit nonzero. Expected linker and trap failures
are recorded baseline matches, not an early shell exit or portability success.
A changed result from fresh source must be investigated, never silently blessed.

A separate future `acceptance` mode must require traps to diagnose at the source
site and exit 1, plus the signal/stack/thread tests in the
[qualification gates](evidence-and-gates.md#staged-go-no-go-criteria). It must not
inherit the baseline's expected SIGILL as a pass. This proposal adds no harness,
executes none of these cases, and claims no new experiment result.
