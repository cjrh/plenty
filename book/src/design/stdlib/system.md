# `std.system`

CPU availability estimates and bounded Linux system-information snapshots.
Snapshots are implemented in Plenty over [`std.fs`](fs.md) and
[`std.io`](io.md). A small native bridge queries platform CPU availability.
There is no subprocess, background sampler, monitoring dependency or global cache.

## Available parallelism

```text
available_parallelism() -> Result[Positive[i64], SystemError]
```

This is an estimate of usable CPU parallelism for sizing a worker pool, distinct
from online logical CPUs (`Procfs.cpu_count()`) and physical cores (not exposed).
Success is always positive and can be passed directly to `ThreadPoolExecutor`.
Query once when configuring a pool; the result is recomputed on each call and
can change as affinity and resource limits change. It is not a reservation,
thread-creation guarantee, utilization measurement or hot-loop operation.

```plenty
import std.system

def main() -> Result[(), IoError]:
    # One worker is this application's deliberate fallback, not library policy.
    workers = std.system.available_parallelism().unwrap_or(1)
    print("positive worker count:", workers.get() > 0)?
    Ok(())
```
```output
positive worker count: True
```

Keep an explicit user-configured count when supplied. CPU-bound workloads often
start with this estimate; I/O-bound workloads may need a different measured
count. See [executor sizing](../../tutorial/11-concurrency/executors-and-futures.md).
The function takes no owned input, allocates no heap storage and starts no
threads. Linux uses bounded stack buffers (less than 160 KiB total), caps
membership/controller files at 65536 bytes, quota files at 512 bytes and paths
at 4096 bytes, and closes every temporary descriptor. It returns `Limit` on
exhaustion, `Unavailable` on inaccessible/unknown data, and `Unsupported` when
there is no backend. It never silently substitutes a CPU count on failure.

| Native release target | Estimate and limitations |
| --- | --- |
| `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | Calling thread's affinity mask, capped by visible cgroup CPU quota. A pinned thread may undercount what the whole process could use. Affinity masks up to 65536 CPUs are supported; a larger required mask returns `Limit`. |
| `aarch64-apple-darwin` | Rust's native available-parallelism estimate; no Linux cgroup quota semantics. |
| `x86_64-pc-windows-msvc` | Rust's native estimate; systems with more than 64 logical CPUs may be undercounted by processor groups, while process affinity/job-object limits may be overcounted. |
| Targets outside Linux, Apple and Windows | `Unsupported`; backend presence does not add compiler/release support for new targets. |

Linux reads `/proc/self/cgroup` and conventional cgroup mounts:
`/sys/fs/cgroup` for v2 and `/sys/fs/cgroup/cpu` or
`/sys/fs/cgroup/cpu,cpuacct` for v1. It takes the minimum visible ancestor quota,
floored to whole CPUs with a minimum of one for fractional quotas. Missing
`cpu.max` files in an existing v2 hierarchy mean no quota at that level, as when
the CPU controller is not enabled. Nonstandard mountpoints, escaped or
namespace-relative parent paths and unavailable membership/hierarchy reads
return `Unavailable`. Hidden ancestors outside a cgroup namespace can still
impose an unobservable tighter quota. VM oversubscription, unequal cores,
simultaneous multithreading, scheduler competition, CPU shares and thread-count
resource limits are not modelled. No call can promise exact effective capacity.

The Linux bridge avoids Rust's allocating cgroup-discovery implementation;
Apple/Windows use its scalar native backend. The [Rust API's platform
limitations](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html#limitations)
remain relevant. Native Linux and deterministic quota fixtures are tested;
Apple/Windows execution requires their native qualification environments.

## Procfs sources

```text
local() -> Result[Procfs, IoError]
open_procfs(path: &std.path.Path) -> Result[Procfs, IoError]
```

`local()` opens `/proc`. `open_procfs` opens another procfs mount or a directory
of captured files; this also supports deterministic tests. `Procfs` owns a
directory handle and closes it on drop. It resolves reads relative to that
handle, following the rooted filesystem API's no-follow rules.

```plenty
import std.system
from std.path import from_text
from std.system import SystemError

def main() -> Result[(), IoError | SystemError]:
    # A captured procfs file gives fixed values; std.system.local() reads /proc.
    write_text("loadavg", "0.50 0.25 0.10 2/180 4242\n")?
    source = std.system.open_procfs(&from_text(".")?)?
    load = source.load_average()?
    print(load.one_minute, load.runnable_tasks, load.last_pid)?
    live = std.system.local()?
    print("CPUs found:", live.cpu_count()? > 0)?
    Ok(())
```
```output
0.5 2 4242
CPUs found: True
```

## Queries

Every method returns `Result[T, SystemError]`. Calls read fresh data; two calls
are not one atomic snapshot. Limits are named optional arguments.

| Method | Result | Source | Defaults |
| --- | --- | --- | --- |
| `cpu_count()` | `i64` online logical CPU records | `stat` | `max_bytes=1048576` |
| `memory()` | `Memory` | `meminfo` | `max_bytes=65536` |
| `load_average()` | `LoadAverage` | `loadavg` | `max_bytes=4096` |
| `uptime()` | `Uptime` | `uptime` | `max_bytes=4096` |
| `block_devices()` | `list[BlockDevice]` | `partitions` | `max_bytes=1048576`, `max_devices=4096` |
| `disk_io()` | `list[DiskIo]` | `diskstats` | `max_bytes=1048576`, `max_devices=4096` |
| `process_ids()` | sorted `list[u32]` | numeric directory entries | `max_entries=65536` |
| `process(pid: u32)` | `Process` | `PID/status` | `max_bytes=65536` |

The generated [signature index](signatures.md#stdsystem) lists every public
record field. For example:

```text
source = std.system.local()?
memory = source.memory(max_bytes=65536)?
print(memory.total_bytes, memory.available_bytes)?
```

## Units and meaning

- `Memory` reports total/free physical bytes, optional available physical bytes,
  and total/free swap bytes. `MemAvailable` is a kernel estimate; `Nothing`
  means the field is unavailable, not zero. Free memory excludes reclaimable
  caches and is usually not the useful estimate of allocation headroom.
- `LoadAverage` has 1-, 5-, and 15-minute load values, runnable/total task counts,
  and the last created PID. Load is not CPU-utilization percent.
- `Uptime.seconds` is elapsed boot time, including suspend according to procfs.
  `idle_cpu_seconds` sums idle time over CPUs and can exceed elapsed time.
- `BlockDevice` gives major/minor device numbers, a name, and `size_bytes`.
  Whole disks **and partitions** can appear; summing them double-counts storage.
  `/proc/partitions` reports 1024-byte blocks, so capacity has that granularity.
  This is not mounted filesystem capacity or free space.
- `DiskIo` gives reads/writes completed, bytes read/written, currently in-flight
  requests, and busy milliseconds. Sector counts are converted using Linux's
  fixed 512-byte accounting unit. These are raw counters, not rates. Counters
  can reset on reboot/device replacement or wrap; detect resets before computing
  differences. Whole disks and partitions overlap here too. New trailing
  diskstats fields are ignored; the modern eleven-counter prefix is required.
- `Process` gives PID, parent PID, the kernel-reported name and state code, real
  UID, thread count, and optional virtual/resident bytes. Name escaping from
  `status` is preserved, including literal backslash escapes; it is not a
  command line or executable path. Missing memory fields, for example on kernel
  threads, yield `Nothing`. Resident memory is an approximate kernel counter.

`Procfs.cpu_count()` is **not** an affinity/cgroup-aware worker-pool size; use
[`available_parallelism`](#available-parallelism) for an estimate. Memory,
load, uptime and devices describe what the mounted files expose, often the
host even in a container. PIDs follow the mount's PID namespace and permissions.
This API does not infer a container's effective CPU or memory quota.

## Limits, failures, and races

`max_bytes` is a `Positive[i64]`; `i64.MAX` traps, as does `process` with pid
`0`. Entry and device bounds are `NonNegative[i64]`. A byte-limited read probes one extra byte to distinguish
exact-limit EOF from truncation, since proc files often report size zero.
Exceeding any bound returns `Limit`, never a partial successful result.
`process_ids` counts **all** visited directory entries toward its bound,
including non-PID names. Parsing and result storage allocate fallibly; byte
limits bound input size, not exact peak memory.

```text
enum SystemError:
    Io(IoError)
    Stream(std.io.StreamError)
    Allocation(AllocError)
    Limit
    Malformed(str)
    Unavailable
    Unsupported
```

I/O errors keep their original cause, including missing and inaccessible files.
Allocation errors can also occur inside the existing `Io`/`Stream` envelopes.
Invalid UTF-8, missing required fields, duplicate selected keys, invalid numbers
and unit-conversion overflow return `Malformed(field_or_source)`. Unknown
meminfo/status keys are ignored. Numeric conversions never truncate or wrap.
Scaling checks the full `u64` product and preserves the field-specific
`Malformed` error on overflow; counts and byte-limit probe offsets are bounded
by their validated limits rather than saturated.

Enumerating PIDs does not read each process's status or silently skip failures.
Call `process(pid)` for the records wanted, choosing how to handle disappearance
or access denial. It opens the PID directory once before reading status, avoiding
mixing files from a reused path; the process may still exit and the read may fail.
A returned PID is neither a stable identity nor authority to signal a process.

Snapshots have no CPU-utilization sampling, filesystem free-space
query, command-line decoding, general cgroup accounting, or process control. For children
owned by the program, use [`std.process`](process.md).

Formats follow the kernel's [procfs documentation](https://docs.kernel.org/filesystems/proc.html),
[disk I/O accounting](https://docs.kernel.org/admin-guide/iostats.html), and
[partitions ABI](https://man7.org/linux/man-pages/man5/proc_partitions.5.html).
See the [tutorial](../../tutorial/08-resources/system-information.md).
The [coverage and backend assessment](system-backends.md) compares this scope
with psutil-style inspection.

## Acknowledgements

- [Rust's `std::thread::available_parallelism`](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html) models the CPU-availability API and supplies the Apple/Windows backend. The embedded Rust standard library is covered by the release's existing MIT/Apache-2.0 notices; no upstream source is copied here.
