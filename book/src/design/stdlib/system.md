# `std.system`

Bounded Linux system-information snapshots, implemented in Plenty over
[`std.fs`](fs.md) and [`std.io`](io.md). No subprocess, background sampler,
native dependency, or global cache is involved.

```text
local() -> Result[Procfs, IoError]
open_procfs(path: &std.path.Path) -> Result[Procfs, IoError]
```

`local()` opens `/proc`. `open_procfs` opens another procfs mount or a directory
of captured files; this also supports deterministic tests. `Procfs` owns a
directory handle and closes it on drop. It resolves reads relative to that
handle, following the rooted filesystem API's no-follow rules.

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

The CPU count is **not** an affinity/cgroup-aware worker-pool size. Memory,
load, uptime and devices describe what the mounted files expose, often the
host even in a container. PIDs follow the mount's PID namespace and permissions.
This API does not infer a container's effective CPU or memory quota.

## Limits, failures, and races

`max_bytes` must be positive and less than `i64.MAX`; entry/device bounds may
be zero but not negative. A byte-limited read probes one extra byte to distinguish
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
    InvalidInput
    Limit
    Malformed(str)
```

I/O errors keep their original cause, including missing and inaccessible files.
Allocation errors can also occur inside the existing `Io`/`Stream` envelopes.
Invalid UTF-8, missing required fields, duplicate selected keys, invalid numbers
and unit-conversion overflow return `Malformed(field_or_source)`. Unknown
meminfo/status keys are ignored. Numeric conversions never truncate or wrap.

Enumerating PIDs does not read each process's status or silently skip failures.
Call `process(pid)` for the records wanted, choosing how to handle disappearance
or access denial. It opens the PID directory once before reading status, avoiding
mixing files from a reused path; the process may still exit and the read may fail.
A returned PID is neither a stable identity nor authority to signal a process.

This first interface has no CPU-utilization sampling, filesystem free-space
query, command-line decoding, cgroup accounting, or process control. For children
owned by the program, use [`std.process`](process.md).

Formats follow the kernel's [procfs documentation](https://docs.kernel.org/filesystems/proc.html),
[disk I/O accounting](https://docs.kernel.org/admin-guide/iostats.html), and
[partitions ABI](https://man7.org/linux/man-pages/man5/proc_partitions.5.html).
See the [tutorial](../../tutorial/08-resources/system-information.md).
