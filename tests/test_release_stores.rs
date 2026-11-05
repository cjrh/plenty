//! Release builds run Cranelift's store elimination and redundant-load
//! passes (`opt_level=speed`; dead- and idempotent-store elimination since
//! 0.137). These programs pass data between threads, through generator
//! frames, and across runtime calls, and must print the same in debug and
//! release builds. Issue #441 records the audit; the coverage-counter case is
//! in test_fuzz.rs. Also covers Cranelift's 1 GiB frame limit.

mod support;

use rstest::rstest;
use std::process::{Command, Output};

/// Compile `source` with the compiled-function cache at `cache` ("off" or a
/// directory) and return the compiler's output and the executable's path.
fn compile(
    dir: &std::path::Path,
    source: &str,
    release: bool,
    cache: &std::ffi::OsStr,
) -> (Output, std::path::PathBuf) {
    let path = dir.join("main.plenty");
    let executable = dir.join("main");
    let unchanged = match std::fs::read_to_string(&path) {
        Ok(previous) => previous == source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => panic!("read {path:?}: {error}"),
    };
    if !unchanged {
        std::fs::write(&path, source).unwrap();
    }
    let mut command = Command::new(support::plenty_bin());
    command
        .env("PLENTY_CACHE", if cache == "off" { "off" } else { "on" })
        .env("PLENTY_CACHE_DIR", cache)
        .arg("--cache-stats");
    if !release {
        command.arg("--fast-compile");
    }
    let output = command
        .arg("--compile")
        .arg(&path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    (output, executable)
}

fn run(source: &str, release: bool) -> String {
    let dir = tempfile::tempdir().unwrap();
    let (built, executable) = compile(dir.path(), source, release, "off".as_ref());
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// A worker's overwritten store through `&mut`, its result handed back by
/// join, and a value published after an atomic flag.
const THREADS: &str = r#"from std.atomic import Atomic, Ordering

def fill(out: &mut i64, value: i64) -> Result[list[i64], AllocError]:
    *out = 1
    *out = value
    mut values: list[i64] = []?
    for i in range(value):
        values.append(i)?
    Ok(values)

def wait_then_publish(ready: &Atomic[bool], seen: &Atomic[i64]) -> Result[i64, OverflowError[i64]]:
    mut spins = 0
    while not ready.load(Ordering.Acquire):
        spins = spins + 1
    seen.store(7, Ordering.Release)
    Ok(1)

def main() -> Result[(), AllocError | IoError | SpawnError[()] | OverflowError[i64]]:
    mut a = 0
    mut b = 0
    with spawn(fill, &mut a, 5)? as first, spawn(fill, &mut b, 9)? as second:
        print(len(first.join()?) + len(second.join()?))?
    print(a)?
    print(b)?
    ready = Atomic(False)
    seen = Atomic[i64](0)
    with spawn(wait_then_publish, &ready, &seen)? as task:
        ready.store(True, Ordering.Release)
        print(task.join()?)?
    print(seen.load(Ordering.Acquire))?
    Ok(())
"#;

/// Channel sends, two updates per lock of a mutex shared by four threads,
/// and executor results.
const CHANNELS: &str = r#"from std.sync import Mutex

def produce(sender: Sender[i64], start: i64) -> Result[(), SendError[i64] | OverflowError[i64]]:
    for i in range(start, start + 1000):
        sender.send(i)?
    Ok(())

def collect(receiver: &Receiver[i64]) -> Result[i64, OverflowError[i64]]:
    mut total = 0
    while True:
        match receiver.recv():
            case Ok(n):
                total = total + n
            case Err(_):
                return Ok(total)
    Ok(total)

def count(stats: &Mutex[i64], rounds: i64) -> Result[(), OverflowError[i64]]:
    for _ in range(rounds):
        with stats.lock() as value:
            *value = *value + 1
            *value = *value + 2
    Ok(())

def square(value: i64) -> Result[i64, OverflowError[i64]]:
    Ok(value * value)

def main() -> Result[(), AllocError | FutureError | IoError | PoolError | SubmitError[OnceClosure[[], Result[i64, OverflowError[i64]]]] | SendError[i64] | SpawnError[()] | OverflowError[i64]]:
    sender, receiver = channel[i64](4)?
    with spawn(produce, sender, 0).drop_job()? as producer:
        with receiver as inbox:
            total = collect(inbox)?
            print(total)?
        producer.join()?
    stats = Mutex(0)
    with spawn(count, &stats, 50000)? as first, spawn(count, &stats, 50000)? as second, spawn(count, &stats, 50000)? as third:
        count(&stats, 50000)?
        first.join()?
        second.join()?
        third.join()?
    print(stats.into_inner())?
    with ThreadPoolExecutor(3, 4)? as pool:
        mut sum = 0
        for n in range(20):
            sum = sum + (pool.submit(square, n)?.result()?)?
        print(sum)?
    Ok(())
"#;

/// Generator state stored before each yield and reloaded on resume, and
/// record values yielded through the caller's output slot.
const GENERATORS: &str = r#"class Pair:
    left: i64
    right: str

def steps(n: i64) -> Generator[Result[i64, OverflowError[i64]]]:
    mut total = 0
    mut i = 0
    while i < n:
        added = checked:
            total + i
        match added:
            case Ok(value):
                total = value
            case Err(error):
                yield Err(error)
                return
        yield Ok(total)
        doubled = checked:
            total * 2
        match doubled:
            case Ok(value):
                total = value
            case Err(error):
                yield Err(error)
                return
        yield Ok(total)
        # 0 <= i < n <= i64.MAX, so advancing the index cannot overflow.
        unchecked:
            i = i + 1

def pairs(n: i64) -> Generator[Result[Pair, OverflowError[i64]]]:
    for i in range(n):
        yield Ok(Pair(i, "p"))
        result = checked:
            Pair(i * 10, "q")
        yield result

def step_option(value: Option[Result[i64, OverflowError[i64]]]) -> Result[Option[i64], OverflowError[i64]]:
    match value:
        case Some(result):
            Ok(Some(result?))
        case Nothing:
            Ok(Nothing)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut sum = 0
    for value in steps(6):
        sum = sum * 3 + value?
    print(sum)?
    mut it = steps(3)
    print(step_option(next(&mut it))?)?
    print(step_option(next(&mut it))?)?
    print(step_option(next(&mut it))?)?
    mut checksum = 0
    for result in pairs(4):
        pair = result?
        checksum = checksum * 7 + pair.left + len(pair.right)
    print(checksum)?
    Ok(())
"#;

#[rstest]
#[case::threads(THREADS, "14\n5\n9\n1\n7\n")]
#[case::channels(CHANNELS, "499500\n600000\n2470\n")]
#[case::generators(
    GENERATORS,
    "53295\nOption[i64].Some(0)\nOption[i64].Some(0)\nOption[i64].Some(1)\n1003334\n"
)]
fn debug_and_release_agree(
    #[case] source: &str,
    #[case] expected: &str,
    #[values(false, true)] release: bool,
) {
    assert_eq!(run(source, release), expected);
}

/// Generator frames live inline in their owner and are bounded by the
/// frontend only at i32::MAX, so nesting reaches Cranelift's 1 GiB frame
/// limit with a short program: `main` holds `g7`, about 1.2 GiB.
fn oversized_frame() -> String {
    let mut source = String::from(
        "class Wide:\n    a: (i64, i64, i64, i64, i64, i64, i64, i64)\n    \
         b: (i64, i64, i64, i64, i64, i64, i64, i64)\n\n\
         def g0() -> Generator[i64]:\n    \
         w = Wide((1, 2, 3, 4, 5, 6, 7, 8), (1, 2, 3, 4, 5, 6, 7, 8))\n    yield 1\n\n",
    );
    for level in 1..=7 {
        source.push_str(&format!("def g{level}() -> Generator[i64]:\n"));
        let inner = if level == 7 { 6 } else { 8 };
        for local in 0..inner {
            source.push_str(&format!("    x{local} = g{}()\n", level - 1));
        }
        source.push_str("    yield 1\n\n");
    }
    source.push_str(
        "def main() -> Result[i32, OverflowError[i64]]:\n    mut t = 0\n    for v in g7():\n        t = t + v\n    Ok(i32(t))\n",
    );
    source
}

#[rstest]
fn frame_over_one_gib_is_a_diagnostic(#[values(false, true)] release: bool) {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let source = oversized_frame();
    let edited = source.replacen("    yield 1\n", "    yield 2\n", 1);
    assert_ne!(source, edited);
    for (phase, source, cache) in [
        ("off", &source, "off".as_ref()),
        ("cold", &source, cache.as_os_str()),
        ("warm", &source, cache.as_os_str()),
        ("edited", &edited, cache.as_os_str()),
    ] {
        let (output, _) = compile(dir.path(), source, release, cache);
        assert!(!output.status.success(), "{phase}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(
                "compiling `main`: its native stack frame exceeds the 1 GiB limit; \
                 store large values in a `Box` or split the function"
            ),
            "{phase}: {stderr}"
        );
    }
}

#[rstest]
fn generator_stores_survive_cold_warm_and_edited_cache(#[values(false, true)] release: bool) {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let edited = GENERATORS.replace("Pair(i, \"p\")", "Pair(i, \"pp\")");
    assert_ne!(GENERATORS, edited);
    let original_output =
        "53295\nOption[i64].Some(0)\nOption[i64].Some(0)\nOption[i64].Some(1)\n1003334\n";
    let edited_output =
        "53295\nOption[i64].Some(0)\nOption[i64].Some(0)\nOption[i64].Some(1)\n1844034\n";
    // Program-cache admission stores a marker on first use, then the object
    // on second use. That second build must reuse every function entry.
    // Optimized builds inline two small helpers into their callers, leaving
    // seven entries.
    let functions = if release { 7 } else { 9 };
    let cold = format!("compile cache: program miss, 0 hits, {functions} misses");
    let admission = format!("compile cache: program miss, {functions} hits, 0 misses");
    for (phase, source, mode, statistic, expected) in [
        (
            "cold-marker",
            GENERATORS,
            cache.as_os_str(),
            cold.as_str(),
            original_output,
        ),
        (
            "object-admission",
            GENERATORS,
            cache.as_os_str(),
            admission.as_str(),
            original_output,
        ),
        (
            "warm",
            GENERATORS,
            cache.as_os_str(),
            "compile cache: program hit",
            original_output,
        ),
        (
            "edited",
            edited.as_str(),
            cache.as_os_str(),
            "compile cache: program miss",
            edited_output,
        ),
        (
            "warm-edited",
            edited.as_str(),
            cache.as_os_str(),
            "compile cache: program hit",
            edited_output,
        ),
        (
            "off-edited",
            edited.as_str(),
            "off".as_ref(),
            "compile cache: off",
            edited_output,
        ),
    ] {
        let (built, executable) = compile(dir.path(), source, release, mode);
        let stderr = String::from_utf8_lossy(&built.stderr);
        assert!(built.status.success(), "{phase}: {stderr}");
        assert!(stderr.contains(statistic), "{phase}: {stderr}");
        println!("release={release} {phase}: {stderr}");
        let output = Command::new(executable).output().unwrap();
        assert!(output.status.success(), "{phase}: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected, "{phase}");
    }
}
