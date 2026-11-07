# System inspection coverage and backends

This assessment accompanies issue #716. It records implemented coverage and
backend tradeoffs, not compatibility with Python's complete
[psutil API](https://psutil.io/api/). Scheduled remaining work belongs to B38 in
the [backlog](../../backlog.md).

## Coverage and platform matrix

“Linux” means the existing bounded `Procfs` queries, with mount permissions and
namespace limitations documented by [`std.system`](system.md).
Apple/Windows CPU availability has native backends; other inspection remains
Linux-only. Native execution is qualified separately from API/backend presence.

| Read-only capability | Plenty on Linux | Plenty on macOS / Windows | Relevant psutil-style distinction |
| --- | --- | --- | --- |
| Worker-count estimate | `available_parallelism`, affinity and visible conventional cgroup quotas | Native Rust estimate, with platform limits | Available parallelism differs from logical and physical counts |
| CPU topology | Online logical records via `Procfs.cpu_count` | Not exposed | Physical cores, packages, NUMA and per-core frequency are distinct |
| CPU utilization | Not exposed; load average is available | Not exposed | Needs two CPU-time snapshots and a caller-selected elapsed interval |
| Physical memory/swap | Total/free, optional available bytes, swap total/free | Not exposed | Available memory differs from free memory; host totals differ from container limits |
| Process information | Bounded visible PIDs, status/name/parent/UID/threads and optional virtual/resident bytes | Not exposed | PID reuse, permissions and exit races; no stable process identity or full command line |
| Disks/filesystems | Block capacities and cumulative disk-I/O counters | Not exposed | Device/partition overlap; filesystem free space and mount enumeration are separate |
| Network inspection | Not exposed by `std.system` | Not exposed | Interface counters differ from addresses, link state and socket/connection enumeration |
| Uptime/load | Procfs uptime and 1/5/15-minute load | Not exposed | Load is runnable/blocked work, not CPU utilization |

The small worker-count query neither refreshes process tables nor samples every
CPU. It adds no hidden polling. Process control is a separate concern: use
[`std.process`](process.md) for children started by the program; a monitoring
snapshot is not authority to signal arbitrary PIDs.

## Backend evaluation

Reviewed against published `psutil` 5.4.0 and `sysinfo` 0.40.1 documentation and
upstream manifests on 2026-11-07. Version-specific source links anchor the
assessment where published sources are available; upstream `master` links are
moving evidence and must be rechecked before adopting a dependency.

| Criterion | Rust `psutil` | Rust `sysinfo` |
| --- | --- | --- |
| Maintenance | The upstream [README](https://github.com/rust-psutil/rust-psutil#support) explicitly warns of irregular maintenance and an unstable API. | The [changelog](https://github.com/GuillaumeGomez/sysinfo/blob/master/CHANGELOG.md) records ongoing platform fixes, API migrations and releases through 0.40.1. This is evidence of activity, not a support guarantee. |
| Platforms | Primarily Linux and partial macOS; the [per-function matrix](https://docs.rs/crate/psutil/5.4.0/source/platform-support.md) has many gaps and only isolated Windows entries. It is not Python psutil's platform coverage. | The [crate documentation](https://docs.rs/sysinfo/0.40.1/sysinfo/) lists Linux, Windows, macOS, Android, iOS, FreeBSD and NetBSD. Individual features still vary, with Apple sandbox/store restrictions. |
| License | MIT, per the [5.4.0 manifest](https://docs.rs/crate/psutil/5.4.0/source/Cargo.toml). | MIT, per the [manifest](https://github.com/GuillaumeGomez/sysinfo/blob/master/Cargo.toml). |
| Dependencies | Default subsystems include CPU/disk/host/memory/network/process/sensors. CPU adds `num_cpus` and `glob`; base dependencies include `nix`, `once_cell` and `thiserror`; Apple paths add native bindings. Features can narrow the graph. | Default features include component/disk/GPU/network/system/user; `system` includes `memchr`, with libc on Unix and larger platform-binding graphs on Apple/Windows. Rayon is opt-in through `multithread`, disabled by default. |
| Refresh and sampling | Stateful collectors retain prior samples, such as `CpuPercentCollector`; not a stateless port of every Python call. See the [API notes](https://docs.rs/psutil/5.4.0/psutil/). | Explicit `refresh_*`/`refresh_specifics`; CPU usage needs at least two observations with the documented minimum interval. Retaining one `System` avoids repeatedly constructing process/CPU state. See the [refresh API](https://docs.rs/sysinfo/0.40.1/sysinfo/struct.System.html). |
| Allocation contract | Ordinary Rust collection/string allocation is not Plenty's recoverable, input-capped `AllocError` model. Returning a backend `Result` alone does not establish OOM recovery. | 0.40's fallible constructors do not make every scan/allocation fallible: the [Linux implementation](https://github.com/GuillaumeGomez/sysinfo/blob/master/src/unix/linux/system.rs) still grows ordinary buffers/strings and owns a process `HashMap`. A wrapper needs an allocation audit rather than treating `Error` as `AllocError`. |

Neither monitoring crate is added. The immediate query uses a small scheduling
bridge; cgroup and affinity operations have enough OS-specific detail to justify
the native boundary. Existing small text-file snapshot parsers remain Plenty.
Wrapping either crate would require feature-specific portability value,
per-operation byte/record limits, recoverable allocation throughout the reachable
backend and precise missing/unsupported/error mapping. A broad native scan with
an output truncation step would not bound its internal allocations.

The dependency and binary-size costs depend on selected features, target and
link-time dead stripping. No end-to-end byte delta is claimed: neither backend
was linked for this assessment. This change adds no Cargo dependency and no
all-system refresh. A later backend proposal should compare minimal standalone
binaries for the exact queries and targets against the native/Plenty baseline.

## Incremental boundaries

CPU-time counters plus an explicit difference operation, bounded interface byte
counters and a filesystem-space query are useful independent slices. Counter
APIs must expose resets, unsupported fields and cumulative units. Utilization
and rates need two snapshots and an explicit elapsed duration; they must not
hide sleeping or background sampling inside an ordinary getter. CPU topology
and richer process snapshots need their own identity, permissions and bounds
contracts. This assessment does not schedule or implement those slices; B38
retains their remaining scope.
