# ARM fault diagnostics

Use this procedure when the ARM integration harness itself exits on a signal.
The [Linux AArch64 guide](linux-aarch64.md) describes the execution environment
and qualification limits. Local issue #592 records a historical SIGILL whose
exact executable and core were not retained; a successful later run cannot
identify its cause. Do not infer the faulting test from the last printed `ok`:
parallel tests can still be running when that line appears.

## Preserve the run before rebuilding

Stop other writers to the task's build and cache directories. Create a fresh,
absolute evidence directory outside the sysroot and build output. Preserve:

- The exact integration ELF, matching `plenty`/`plentyc`, runtime archives,
  optional engines, and split debug files (`.dwo`). Copy bytes, not hard links
  to artifacts a build can overwrite. Record SHA-256 hashes and ELF build IDs.
- The source commit, `Cargo.lock`, feature flags, build command, tracked changes
  (`git diff --binary HEAD`) and any untracked source inputs. A revision alone
  does not describe a dirty build. Keep the source checkout available: tests
  can embed its absolute paths and read fixtures from it.
- The complete toolchain/sysroot and tool-wrapper scripts, including symlink
  targets, plus QEMU and debugger versions. Retain the shared libraries reported
  by the debugger. A hash without the corresponding file is not a preserved
  artifact.
- Cache contents before execution, and a separate working copy for the rerun.
  Keep the post-run working copy too. Do not clear or reuse the original cache.
- The exact command, working directory, CPU affinity, test threads, environment
  overrides, inherited core limits and raw stdout/stderr. Record opt-ins and
  skipped tests; a passing count alone does not establish coverage.

Before a rerun, verify the ELF's `--list` output, captured source/compiler paths,
subprocess execution and linker setup. An explicit QEMU command wraps only the
initial process: it does not make nested ARM executables runnable. Preserve
existing binfmt or guest configuration as part of the run description. If that
configuration is gone, restoring it is a new experiment, not an exact replay.

## Capture the guest fault with GDB

This method captures guest registers and memory without changing host-wide
`kernel.core_pattern` or relying on the shell's core limit. It was checked with
QEMU 10.2.2 and an AArch64-capable GDB 18.1. The GDB stub changes timing, so retain
ordinary-run evidence as well. Use a short, task-owned Unix socket path; local
sandbox policies may require permission to create/connect to that socket.

Set these absolute paths for the preserved run (the values are examples):

```sh
ARM_EVIDENCE=/tmp/plenty-arm-fault-001
ARM_ELF="$ARM_EVIDENCE/integration"
ARM_SYSROOT="$ARM_EVIDENCE/toolchain/aarch64-buildroot-linux-gnu/sysroot"
```

From the original test working directory, launch the saved ELF with the original
environment, affinity and test arguments, adding QEMU's `-g` option. For a
four-thread Cortex-A53 run the command has this form:

```sh
qemu-aarch64 -cpu cortex-a53 -L "$ARM_SYSROOT" \
    -g "$ARM_EVIDENCE/gdb.sock" "$ARM_ELF" \
    --test-threads=4 --nocapture > "$ARM_EVIDENCE/run.log" 2>&1
```

It waits for a debugger. In another terminal, open the **same ELF** with an
AArch64-capable `gdb` (some distributions call it `gdb-multiarch`):

```sh
gdb -q -iex 'set debuginfod enabled off' "$ARM_ELF"
```

Enter these GDB commands, substituting the absolute evidence paths. GDB command
files do not expand shell variables such as `$ARM_EVIDENCE`.

```text
set pagination off
set sysroot /tmp/plenty-arm-fault-001/toolchain/aarch64-buildroot-linux-gnu/sysroot
set logging file /tmp/plenty-arm-fault-001/gdb.txt
set logging overwrite off
set logging enabled on
target remote /tmp/plenty-arm-fault-001/gdb.sock
handle SIGILL stop print pass
continue
```

When the debugger reports SIGILL, capture before continuing:

```text
info registers
x/8i $pc
x/32bx $pc
thread apply all bt full
info sharedlibrary
info proc mappings
generate-core-file /tmp/plenty-arm-fault-001/guest.core
```

Some QEMU/GDB combinations do not expose process mappings or core generation.
Retain their errors along with the register, instruction and backtrace output;
do not claim a core was captured unless it exists and can be reopened. Even a
readable core can lack mappings or debugging information. For a generated core,
check it independently against the saved ELF:

```sh
gdb -q -iex 'set debuginfod enabled off' "$ARM_ELF" "$ARM_EVIDENCE/guest.core"
```

Compare `info registers pc sp` and `x/i $pc` with the live capture. Preserve the
loaded objects and sysroot alongside the core. After capture, `continue`
delivers the stopped signal normally; Plenty uses intentional illegal
instructions for checked traps, so a SIGILL stop alone does not prove a defect.
Record whether it is handled or terminates the process. The stub observes the
attached process; a subprocess's failure needs its own capture.

## Interpret and minimize

Identify the instruction and containing ELF/object before attributing a cause.
Compare with the compiler's registered trap sites and the runtime signal handler.
Keep the first captured failure, then reduce test filters and thread counts in
separate runs with copied inputs. Compare cache-off, fresh-cache and preserved
cache runs explicitly. Debugger timing or a serial pass does not disprove a race.

Reconcile the failing revision with intervening compiler/runtime fixes. Compare
the minimized case in user-mode QEMU, the ARM Linux guest and native ARM when
available, preserving the same assertions. Record non-reproduction and missing
environments honestly; diagnostic improvements do not resolve an unexplained
historical crash.
