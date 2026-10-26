//! Bounded Linux procfs snapshots, with synthetic files for deterministic
//! parsing, units, limits and errors, plus a real procfs smoke check.
#![cfg(target_os = "linux")]

mod support;

use std::path::Path;
use std::process::Command;

const MEMORY: &str = "MemTotal: 1000 kB\nMemFree: 100 kB\nMemAvailable: 600 kB\nSwapTotal: 20 kB\nSwapFree: 12 kB\nFutureField: anything\n";
const STATUS: &str = "Name:\tworker (with spaces)\\nname\nState:\tS (sleeping)\nPid:\t42\nPPid:\t1\nUid:\t1000\t1001\t1002\t1003\nThreads:\t3\nVmSize:\t2048 kB\nVmRSS:\t512 kB\n";

fn fixture(root: &Path) {
    let procfs = root.join("proc");
    std::fs::create_dir_all(procfs.join("42")).unwrap();
    std::fs::create_dir_all(procfs.join("7")).unwrap();
    for (name, text) in [
        (
            "stat",
            "cpu 100 0 20 500\ncpu0 50 0 10 250\ncpu8 50 0 10 250\nintr 90\n",
        ),
        ("meminfo", MEMORY),
        ("loadavg", "0.50 1.25 2.00 3/200 99\n"),
        ("uptime", "123.5 456.0\n"),
        (
            "partitions",
            "major minor  #blocks  name\n\n 259 0 123456 nvme0n1\n 259 1 1024 nvme0n1p1\n",
        ),
        (
            "diskstats",
            "259 0 nvme0n1 7 1 8 2 9 3 10 4 2 11 12 13 14 15 16 17 18\n",
        ),
        ("42/status", STATUS),
    ] {
        std::fs::write(procfs.join(name), text).unwrap();
    }
}

const DRIVER: &str = r#"import std.system
import std.path
import std.env
from std.system import SystemError

def show[T](value: Result[T, SystemError]) -> Result[(), IoError]:
    match value:
        case Ok(item):
            print("ok", item)?
        case Err(error):
            print("error", error)?
    Ok(())

def main() -> Result[(), IoError | SystemError]:
    source = std.system.open_procfs(&std.path.from_text("proc")?)?
    operation = std.env.get("CASE")?.unwrap_or("memory")
    match operation:
        case "memory":
            show(source.memory())?
        case "cpu":
            show(source.cpu_count())?
        case "load":
            show(source.load_average())?
        case "uptime":
            show(source.uptime())?
        case "blocks":
            show(source.block_devices())?
        case "disk":
            show(source.disk_io())?
        case "ids":
            show(source.process_ids())?
        case "process":
            show(source.process(42u32))?
        case "name":
            print(source.process(42u32)?.name)?
        case "missing":
            show(source.process(999u32))?
        case "bounds":
            show(source.uptime(max_bytes=12))?
            show(source.uptime(max_bytes=11))?
            show(source.uptime(max_bytes=0))?
            show(source.uptime(max_bytes=i64.MAX))?
            show(source.block_devices(max_devices=0))?
            show(source.disk_io(max_devices=-1))?
            show(source.process_ids(max_entries=0))?
            show(source.process(0u32))?
        case _:
            pass
    Ok(())
"#;

fn run(executable: &Path, root: &Path, case: &str) -> String {
    let output = Command::new(executable)
        .current_dir(root)
        .env("CASE", case)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn snapshots_have_explicit_units_and_preserve_process_names() {
    let (dir, executable) = support::build_file(DRIVER);
    fixture(dir.path());
    let memory = run(&executable, dir.path(), "memory");
    for field in [
        "total_bytes=1024000",
        "free_bytes=102400",
        "Some(614400)",
        "swap_total_bytes=20480",
        "swap_free_bytes=12288",
    ] {
        assert!(memory.contains(field), "{memory}");
    }
    assert_eq!(run(&executable, dir.path(), "cpu"), "ok 2");
    assert_eq!(run(&executable, dir.path(), "ids"), "ok [7, 42]");
    let load = run(&executable, dir.path(), "load");
    for field in [
        "one_minute=0.5",
        "five_minutes=1.25",
        "fifteen_minutes=2.0",
        "runnable_tasks=3",
        "total_tasks=200",
        "last_pid=99",
    ] {
        assert!(load.contains(field), "{load}");
    }
    let uptime = run(&executable, dir.path(), "uptime");
    assert!(
        uptime.contains("seconds=123.5") && uptime.contains("idle_cpu_seconds=456.0"),
        "{uptime}"
    );
    let devices = run(&executable, dir.path(), "blocks");
    assert!(
        devices.contains("size_bytes=126418944") && devices.contains("size_bytes=1048576"),
        "{devices}"
    );
    let io = run(&executable, dir.path(), "disk");
    for field in [
        "reads_completed=7",
        "bytes_read=4096",
        "writes_completed=9",
        "bytes_written=5120",
        "in_flight=2",
        "busy_milliseconds=11",
    ] {
        assert!(io.contains(field), "{io}");
    }
    let process = run(&executable, dir.path(), "process");
    for field in [
        "pid=42",
        "parent_pid=1",
        "worker (with spaces)",
        "real_uid=1000",
        "threads=3",
        "Some(2097152)",
        "Some(524288)",
    ] {
        assert!(process.contains(field), "{process}");
    }
    std::fs::write(
        dir.path().join("proc/42/status"),
        STATUS
            .replace("Name:\tworker", "Name:\t worker")
            .replace("\\nname\n", "\\nname \n"),
    )
    .unwrap();
    assert_eq!(
        run(&executable, dir.path(), "name"),
        " worker (with spaces)\\nname "
    );
}

#[test]
fn missing_optional_values_are_not_zero_and_io_errors_stay_visible() {
    let (dir, executable) = support::build_file(DRIVER);
    fixture(dir.path());
    std::fs::write(
        dir.path().join("proc/meminfo"),
        MEMORY.replace("MemAvailable: 600 kB\n", ""),
    )
    .unwrap();
    let result = run(&executable, dir.path(), "memory");
    assert!(
        result.contains("available_bytes=Option[u64].Nothing"),
        "{result}"
    );
    std::fs::write(
        dir.path().join("proc/42/status"),
        STATUS.replace("VmSize:\t2048 kB\nVmRSS:\t512 kB\n", ""),
    )
    .unwrap();
    let result = run(&executable, dir.path(), "process");
    assert!(
        result.contains("virtual_bytes=Option[u64].Nothing")
            && result.contains("resident_bytes=Option[u64].Nothing"),
        "{result}"
    );
    assert!(run(&executable, dir.path(), "missing").contains("IoError.System(2)"));
    std::fs::remove_file(dir.path().join("proc/42/status")).unwrap();
    assert!(run(&executable, dir.path(), "process").contains("IoError.System(2)"));
}

#[test]
fn malformed_overflowing_and_non_utf8_input_is_an_error() {
    let (dir, executable) = support::build_file(DRIVER);
    fixture(dir.path());
    for (name, operation, contents) in [
        (
            "meminfo",
            "memory",
            MEMORY.replace("1000 kB", "18446744073709551615 kB"),
        ),
        ("meminfo", "memory", MEMORY.replace("1000 kB", "1000 MB")),
        ("meminfo", "memory", format!("{MEMORY}MemTotal: 1 kB\n")),
        (
            "meminfo",
            "memory",
            MEMORY.replace("MemTotal: 1000 kB\n", ""),
        ),
        ("stat", "cpu", "cpu 1 2 3\n".into()),
        ("stat", "cpu", "cpu0 1 2 3 4\ncpu0 1 2 3 4\n".into()),
        ("loadavg", "load", "NaN 1 2 3/4 5\n".into()),
        ("loadavg", "load", "1 2 3 4/5/6 7\n".into()),
        ("uptime", "uptime", "inf 0\n".into()),
        ("uptime", "uptime", "-1 0\n".into()),
        (
            "partitions",
            "blocks",
            "major minor #blocks name\n1 2 18446744073709551615 x\n".into(),
        ),
        (
            "diskstats",
            "disk",
            "1 2 x 0 0 18446744073709551615 0 0 0 0 0 0 0 0\n".into(),
        ),
        ("diskstats", "disk", "1 2 x 0 0\n".into()),
        (
            "42/status",
            "process",
            STATUS.replace("Pid:\t42", "Pid:\t41"),
        ),
        (
            "42/status",
            "process",
            STATUS.replace("Threads:\t3", "Threads:\t-1"),
        ),
    ] {
        std::fs::write(dir.path().join("proc").join(name), contents).unwrap();
        let output = run(&executable, dir.path(), operation);
        assert!(output.contains("SystemError.Malformed"), "{name}: {output}");
    }
    std::fs::write(dir.path().join("proc/meminfo"), b"\xff").unwrap();
    assert!(run(&executable, dir.path(), "memory").contains("SystemError.Malformed"));
}

#[test]
fn byte_and_entry_limits_are_enforced_without_truncation() {
    let (dir, executable) = support::build_file(DRIVER);
    fixture(dir.path());
    let output = run(&executable, dir.path(), "bounds");
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), 8, "{output}");
    assert!(lines[0].starts_with("ok "), "{output}");
    for index in [1, 4, 6] {
        assert!(lines[index].contains("SystemError.Limit"), "{output}");
    }
    for index in [2, 3, 5, 7] {
        assert!(
            lines[index].contains("SystemError.InvalidInput"),
            "{output}"
        );
    }
}

#[test]
fn real_procfs_is_readable_without_assuming_machine_sizes() {
    let output = support::run_file_stdout(
        r#"import std.system
from std.system import SystemError
def main() -> Result[(), SystemError | IoError]:
    source = std.system.local()?
    print(source.cpu_count()? > 0)?
    print(source.memory()?.total_bytes > 0u64)?
    print(source.uptime()?.seconds >= 0.0)?
    print(source.load_average()?.one_minute >= 0.0)?
    print(len(source.process_ids()?) > 0)?
    drop(source.block_devices()?)
    drop(source.disk_io()?)
    Ok(())
"#,
    );
    assert_eq!(output.trim(), "True\nTrue\nTrue\nTrue\nTrue");
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_are_values_and_file_descriptors_are_released() {
    let program = r#"import std.system
import std.path
import std.fs
from std.system import SystemError
from std.io import StreamError

def allocation(error: &SystemError) -> bool:
    match error:
        case SystemError.Allocation(_):
            True
        case SystemError.Io(IoError.Data(DataError.Allocation(_))):
            True
        case SystemError.Stream(StreamError.Allocation(_)):
            True
        case SystemError.Stream(StreamError.Io(IoError.Data(DataError.Allocation(_)))):
            True
        case _:
            False

def descriptors() -> Result[i64, IoError]:
    mut entries = std.fs.read_dir(&std.path.from_text("/proc/self/fd")?)?
    mut count = 0
    while entries.next()?.is_some():
        count += 1
    Ok(count)

def attempt(source: &std.system.Procfs, kind: i64) -> Result[(), SystemError]:
    if kind == 0:
        drop(source.memory()?)
    elif kind == 1:
        drop(source.process(42u32)?)
    else:
        drop(source.process_ids()?)
    Ok(())

def main() -> Result[(), IoError | AllocError]:
    source = std.system.open_procfs(&std.path.from_text("proc")?)?
    mut markers = list[str]()?
    for count in range(500):
        markers.append("".join(&["__test_fail_allocations_after_", str.from(count)?, "__"]?)?)?
    before = descriptors()?
    for kind in range(3):
        mut failed = 0
        mut succeeded = False
        mut other = 0
        for marker in &markers:
            print(marker)?
            result = attempt(&source, kind)
            print("__test_restore_allocations__")?
            match result:
                case Ok(_):
                    succeeded = True
                case Err(error):
                    if allocation(&error):
                        failed += 1
                    else:
                        other += 1
        print(failed > 0, succeeded, other == 0)?
    print(before == descriptors()?)?
    Ok(())
"#;
    let (dir, executable) = support::build_file(program);
    fixture(dir.path());
    assert_eq!(
        run(&executable, dir.path(), ""),
        "True True True\nTrue True True\nTrue True True\nTrue"
    );
}
