# Warm script startup measurements

Issue #699 investigated fresh-process launches of an unchanged no-op script.
The expensive work was extracting the bundled runtime before an executable-cache
lookup that did not need it. Deferring that extraction until a miss reduced the
measured shebang median from **16.35 ms to 6.56 ms**, without bypassing source,
toolchain or native-input validation. These are local measurements, not startup
guarantees for other machines or toolchains.

## Workload and environment

Measured on 2026-11-03: AMD Ryzen 7 7840HS, 16 logical CPUs, x86-64 Fedora Linux
7.2.11-200.fc44, glibc 2.43, GCC 16.2.1 and mold 2.40.4. Both compiler binaries
were Plenty 0.8.8, built with Rust 1.99.1 using
`cargo build --locked --release --bin plenty`, without profiling features. The
baseline was `bd407f0525ab8a4da277de543ba60da3b3001b54`; the changed compiler
used the same sources with deferred runtime extraction. The script was:

```plenty
#!/usr/bin/env -S plenty script
def main() -> Result[(), IoError]:
    Ok(())
```

The benchmark used a temporary working directory, an isolated private cache,
tmpfs temporary outputs, and a fixed environment containing only `HOME`,
`PATH`, `PLENTY_CACHE_DIR` and `TMPDIR`. `PATH` contained a benchmark-local
`plenty` symlink and `/usr/bin:/bin`. Redirection/injection variables were absent.
Both program-object and executable hits were confirmed before and after timing;
the linker was not rerun in the timed launches. This establishes the cause of
the reported high system time, but does not reconstruct the original fish
session's compiler build, full environment or exact timing.

## Repeated untraced results

Each workload had ten untimed warm-ups and 100 fresh-process samples per
compiler. Workload order was shuffled within each repetition. Wall time includes
Python's subprocess launch/wait and output collection; child user/system times
come from `getrusage(RUSAGE_CHILDREN)`. Diagnostics, compilation, cache population
and tracing were separate. No fish timing overhead is included.

| Workload | Before median (p10–p90), ms | After median (p10–p90), ms |
| --- | --- | --- |
| `plenty script hello.plenty` | 15.984 (15.237–17.161) | 6.189 (5.686–6.705) |
| Executable env/shebang script | 16.346 (15.597–17.111) | 6.562 (6.097–7.075) |
| Equivalent AOT executable | 0.679 (0.524–0.781) | 0.687 (0.526–0.783) |
| `plenty --version` | 1.152 (0.933–1.319) | 1.118 (0.900–1.285) |
| `/usr/bin/true` | 0.543 (0.440–0.616) | 0.521 (0.425–0.599) |

For direct invocation, median child user/system CPU fell from 2.615/13.114 ms
to 2.657/3.324 ms. For shebang invocation it fell from 2.747/13.330 ms to
2.628/3.741 ms. Removing runtime extraction saved about 9.8 ms per launch;
shebang/PATH handling was a small part of the original delay.

A reverse-order repeat on the final tree (another 100 samples per workload)
measured 17.010 versus 6.650 ms for shebang execution and 16.516 versus
6.179 ms for direct invocation. The improvement persisted when the changed
compiler ran first. Raw reports include compiler SHA-256 identities.

## Attribution and remaining work in a hit

Separate `strace -f -c` and `strace -f -tt -T` runs confirmed the mechanism:

- Before: write a **17,401,052-byte runtime archive** into the temporary
  directory, then remove it after restoring a cached executable. One detailed
  trace measured 7.78 ms for the write and 2.10 ms for its removal. These traced
  syscall times are illustrative; the untraced before/after difference above
  measures the overall saving.
- After: neither open/write nor removal of that archive occurs on a hit.
  The object (10,048 bytes) and restored private executable (121,104 bytes)
  are still materialized. The native-validation trace retained 447 `statx`
  and 222 `readlink` calls; it was not replaced with a stale-path shortcut.
- Both versions invoke GCC for `-print-prog-name=ld.mold` and
  `-print-search-dirs`, then launch/wait for the application. Independent
  untraced probe measurements (100 samples after ten warm-ups) had medians of
  0.782 and 0.854 ms. Those probes remain necessary under the current
  [toolchain eligibility contract](../design/toolchain/02-architecture/cache.md#single-file-executables).
- Compiler startup/dynamic loading has the approximately 1.1 ms `--version`
  control; the AOT child has the approximately 0.7 ms control. The rest of a
  roughly 6.2 ms direct hit includes runner/header/project setup, private locks,
  source/native dependency checks, object/executable restoration and cleanup.
  These independent medians form a rough cost budget, not additive phase
  instrumentation. Syscall attribution was sufficient to select this change;
  no compiler-phase profiler was needed.

Cold misses still extract and link the runtime. Wrappers, redirected toolchains,
explicit outputs, hooks/fuzz builds and other unsupported executable-cache
configurations retain their existing linking behavior. The compiler identity
already includes the embedded runtime; deferring its extraction changes no cache
key or manifest. Existing shared-library runtime files remain explicit inputs.
The regression makes the runtime extraction path a directory: a real warm hit
must succeed without writing it, while an edited-object miss must fail extraction
and leave no executable to run. Cache invalidation, private executable restore,
argument/exit handling and locking retain their existing tests and contracts.

## Reproduction

Preserve a baseline release compiler before rebuilding the changed compiler,
then run the helper from this checkout on an otherwise quiet machine:

```sh
python3 -B scripts/measure_script_startup.py /absolute/before/plenty /absolute/after/plenty \
    --runs 100 --warmup 10 --output /tmp/script-startup.json
```

The helper creates the script, AOT control, isolated cache and fixed environment,
checks hits outside timing, prints distributions and writes raw wall/CPU samples
to JSON. It refuses to benchmark a toolchain that cannot confirm executable hits.
It does not change CPU frequency policy or pin a core. Repeat in reverse compiler
order when checking for drift. Do not run compilation or tracing alongside the
timed comparison.

For syscall diagnostics, create a separate working directory containing the
same `hello.plenty`, warm it twice with the exact same fixed environment, then
use `strace -f -c` or `strace -f -tt -T`. Keep trace output outside that working
directory, since its identity is a watched linker input. Never report traced
wall time as an ordinary launch sample. For a single fixed compiler, for example:

```sh
env -i HOME="$HOME" PATH=/usr/bin:/bin PLENTY_CACHE_DIR=/tmp/script-trace-cache \
    /absolute/plenty script --cache-stats hello.plenty
# Repeat to confirm hits, then trace with the same environment:
env -i HOME="$HOME" PATH=/usr/bin:/bin PLENTY_CACHE_DIR=/tmp/script-trace-cache \
    strace -f -c -o /tmp/script-syscalls.txt /absolute/plenty script hello.plenty
```
