# Read system information

`std.system` reads Linux's `/proc` files into typed values. Open the source once,
then ask for the information needed. Each query is fresh and fallible:

```plenty
import std.system
from std.system import SystemError

def main() -> Result[(), IoError | SystemError]:
    source = std.system.local()?
    cpus = source.cpu_count()?
    memory = source.memory()?
    uptime = source.uptime()?
    print("CPU information available:", cpus > 0)?
    print("Memory information available:", memory.total_bytes > 0u64)?
    print("Uptime available:", uptime.seconds >= 0.0)?
    Ok(())
```
```output
CPU information available: True
Memory information available: True
Uptime available: True
```

The example prints checks so its output stays the same on different machines.
A monitor can print `cpus`, `memory.total_bytes` and `uptime.seconds` directly.
There is no timer or background thread: another call reads another snapshot.

For memory headroom, prefer `available_bytes` to `free_bytes`: the kernel's
available estimate includes memory it expects to reclaim. The field is optional
because not every source provides it. Here is how to keep absence distinct from
zero, using sample values so both paths are visible:

```plenty
import std.system

def show_available(memory: &std.system.Memory) -> Result[(), IoError]:
    match memory.available_bytes:
        case Some(bytes):
            print("available bytes:", bytes)?
        case Nothing:
            print("available memory is not reported")?
    Ok(())

def main() -> Result[(), IoError]:
    reported = std.system.Memory(8192u64, 1024u64, Some(4096u64), 0u64, 0u64)
    absent = std.system.Memory(8192u64, 1024u64, Nothing, 0u64, 0u64)
    show_available(&reported)?
    show_available(&absent)?
    Ok(())
```
```output
available bytes: 4096
available memory is not reported
```

Reads have default byte limits; override them with a named argument such as
`source.memory(max_bytes=65536)`. A file that exceeds the limit produces an error,
not a truncated record. `open_procfs(&path)` reads a different procfs mount or
captured directory, useful when testing a monitoring tool.

`process_ids()` returns a bounded, sorted list of visible process IDs.
`process(pid)` reads one status record. Processes can exit between those calls,
and access can be denied; handle the `SystemError.Io` cause rather than treating
the whole list as guaranteed live. Use [`std.process`](../../design/stdlib/process.md)
to manage children your program starts.

`block_devices()` reports device and partition sizes; `disk_io()` reports
counters. Avoid summing whole disks with their partitions. Neither gives free
filesystem space. CPU and memory values can describe the host even inside a
container; they are not cgroup quota measurements or a recommendation for how
many workers to start.

See [`std.system`](../../design/stdlib/system.md) for fields, units, error
variants and snapshot limitations.
