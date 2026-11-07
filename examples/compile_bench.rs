//! Repeatable compiler-latency probe, without a benchmark framework dependency.
//! cargo run --release --example compile_bench -- 1000 10
//! The arguments are the generated unit count and the repetitions.
//! Add --aot to also measure native emission, archive extraction, and linking.
//! Select just one workload with --workload check|scale|aot; --no-warmup makes
//! the profile contain exactly the requested repetitions.
//! With `--features profile` and PLENTY_PROFILE=1, it also prints a phase
//! profile; see book/src/development/profiling.md.
use std::error::Error;
use std::time::{Duration, Instant};

// mimalloc rather than the system allocator: see the dependency in
// Cargo.toml (issue #315).
#[cfg(not(feature = "profile-alloc"))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

// With `profile-alloc`, every allocation is counted against the measured
// function that makes it (issue #150).
#[cfg(feature = "profile-alloc")]
#[global_allocator]
static ALLOCATOR: hotpath::CountingAllocator<mimalloc::MiMalloc> =
    hotpath::CountingAllocator::with(mimalloc::MiMalloc);

const USAGE: &str = "Usage: compile_bench [UNITS] [REPETITIONS] [--aot]\n\
    [--workload check|scale|aot|check-and-scale|all] [--no-warmup] [--fast-compile]\n\n\
Default: check UNITS and 4*UNITS, with one warm-up per workload.\n\
--aot also compiles and links UNITS (equivalent to --workload all).\n\
--workload scale checks only 4*UNITS; check and aot use UNITS.\n\
Profiles include warm-ups unless --no-warmup is supplied.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Workload {
    Check,
    Scale,
    Aot,
    CheckAndScale,
    All,
}

impl Workload {
    fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Scale => "scale",
            Self::Aot => "aot",
            Self::CheckAndScale => "check-and-scale",
            Self::All => "all",
        }
    }

    fn check(self) -> bool {
        matches!(self, Self::Check | Self::CheckAndScale | Self::All)
    }

    fn scale(self) -> bool {
        matches!(self, Self::Scale | Self::CheckAndScale | Self::All)
    }

    fn aot(self) -> bool {
        matches!(self, Self::Aot | Self::All)
    }
}

#[derive(Debug)]
struct Options {
    units: usize,
    repetitions: usize,
    workload: Workload,
    warmup: bool,
    fast_compile: bool,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut options = Self {
            units: 1000,
            repetitions: 10,
            workload: Workload::CheckAndScale,
            warmup: true,
            fast_compile: false,
        };
        let mut args = args.into_iter();
        let mut positional = 0;
        let mut selected = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--workload" | "--aot" => {
                    if selected {
                        return Err("choose --workload or --aot only once".into());
                    }
                    selected = true;
                    options.workload = if arg == "--aot" {
                        Workload::All
                    } else {
                        match args.next().as_deref() {
                            Some("check") => Workload::Check,
                            Some("scale") => Workload::Scale,
                            Some("aot") => Workload::Aot,
                            Some("check-and-scale") => Workload::CheckAndScale,
                            Some("all") => Workload::All,
                            _ => return Err(
                                "--workload requires check, scale, aot, check-and-scale, or all"
                                    .into(),
                            ),
                        }
                    };
                }
                "--no-warmup" => options.warmup = false,
                "--fast-compile" => options.fast_compile = true,
                _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}").into()),
                _ => {
                    match positional {
                        0 => options.units = arg.parse()?,
                        1 => options.repetitions = arg.parse()?,
                        _ => return Err("expected at most UNITS and REPETITIONS".into()),
                    }
                    positional += 1;
                }
            }
        }
        if options.units == 0 || options.repetitions == 0 {
            return Err("unit count and repetitions must be positive".into());
        }
        if options.workload.scale() && options.units.checked_mul(4).is_none() {
            return Err("four-times unit count overflows usize".into());
        }
        Ok(options)
    }
}

fn measure(
    mut action: impl FnMut() -> Result<(), Box<dyn Error>>,
    repetitions: usize,
    warmup: bool,
) -> Result<Duration, Box<dyn Error>> {
    if warmup {
        action()?; // excluded from timed samples, but included in a profile
    }
    let mut times = Vec::with_capacity(repetitions);
    for _ in 0..repetitions {
        let start = Instant::now();
        action()?;
        times.push(start.elapsed());
    }
    times.sort_unstable();
    Ok(times[times.len() / 2])
}

/// One unit declares a protocol, a class, an enum, and three functions, so
/// every per-declaration pass of the frontend is exercised. Names carry a
/// prefix that no builtin type shares.
fn workload(units: usize) -> String {
    let mut source = String::new();
    for i in 0..units {
        source.push_str(&format!(
            "protocol Measured{i}:
    def squared_length(self) -> Result[i64, OverflowError[i64]]:
        pass

class Point{i}:
    x: i64
    y: i64

    def squared_length(self) -> Result[i64, OverflowError[i64]]:
        Ok(self.x * self.x + self.y * self.y + {i})

    def shift(self: &mut Point{i}, amount: i64) -> Result[(), OverflowError[i64]]:
        self.x = self.x + amount
        self.y = self.y + amount
        Ok(())

enum Reading{i}:
    Missing
    Value(i64)

def describe{i}(reading: Reading{i}) -> Result[i64, OverflowError[i64]]:
    match reading:
        case Reading{i}.Missing:
            Ok(0)
        case Reading{i}.Value(value):
            Ok(value + {i})

def squares{i}(limit: i64) -> Result[list[i64], AllocError | OverflowError[i64] | ZeroDivisionError]:
    Ok([n * n + {i} for n in range(limit) if n % 2 == 0]?)

def work{i}(a: i64) -> Result[i64, AllocError | OverflowError[i64] | ZeroDivisionError]:
    mut point = Point{i}(a, {i})
    point.shift(1)?
    values = squares{i}(a)?
    mut total = point.squared_length()? + describe{i}(Reading{i}.Value(a))?
    for v in &values:
        if v > {i}:
            total = total + v
    Ok(total)

"
        ));
    }
    source.push_str(&format!(
        "def main() -> Result[(), IoError | AllocError | OverflowError[i64] | ZeroDivisionError]:\n    print(work{}(5)?)?\n    Ok(())\n",
        units - 1
    ));
    source
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        println!("{USAGE}");
        return Ok(());
    }
    let options = Options::parse(args)?;
    #[cfg(feature = "profile")]
    // SAFETY: argument parsing above has not spawned any threads.
    let _profile = unsafe { plenty::profile::start() };
    let count = if options.workload == Workload::Scale {
        options.units * 4
    } else {
        options.units
    };
    let source = workload(count);
    println!(
        "profile={} target={}-{} requested_units={} units={} lines={} bytes={} repetitions={} workload={} warmup={} codegen={}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        std::env::consts::ARCH,
        std::env::consts::OS,
        options.units,
        count,
        source.lines().count(),
        source.len(),
        options.repetitions,
        options.workload.name(),
        options.warmup,
        if options.fast_compile {
            "fast-compile"
        } else {
            "optimized"
        }
    );
    let checking = if options.workload.check() {
        let checking = measure(
            || plenty::check_source(&source),
            options.repetitions,
            options.warmup,
        )?;
        println!(
            "parse_resolve_check_median_ms={:.3}",
            checking.as_secs_f64() * 1000.0
        );
        Some(checking)
    } else {
        None
    };
    if options.workload.scale() {
        // A fourfold workload should take roughly four times as long; a ratio
        // near 16 can reveal quadratic work (issue #53).
        let larger = (options.workload != Workload::Scale).then(|| workload(count * 4));
        let scaled = measure(
            || plenty::check_source(larger.as_deref().unwrap_or(&source)),
            options.repetitions,
            options.warmup,
        )?;
        print!(
            "parse_resolve_check_4x_units_median_ms={:.3}",
            scaled.as_secs_f64() * 1000.0
        );
        if let Some(checking) = checking {
            print!(
                " ratio={:.2}",
                scaled.as_secs_f64() / checking.as_secs_f64()
            );
        }
        println!();
    }
    if options.workload.aot() {
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("program");
        // The program cache would serve every timed build after the first
        // without compiling anything; the function cache stays as configured.
        let compile = plenty::CompileOptions {
            program_cache: false,
            optimize: !options.fast_compile,
            ..Default::default()
        };
        let native = measure(
            || plenty::compile_source_to_executable_with_options(&source, &output, &compile),
            options.repetitions,
            options.warmup,
        );
        println!(
            "aot_including_check_archive_link_median_ms={:.3}",
            native?.as_secs_f64() * 1000.0
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> Options {
        Options::parse(args.iter().map(|arg| (*arg).to_owned())).unwrap()
    }

    #[test]
    fn existing_invocations_keep_their_workloads() {
        let default = options(&[]);
        assert_eq!((default.units, default.repetitions), (1000, 10));
        assert_eq!(default.workload, Workload::CheckAndScale);
        assert!(default.warmup);
        assert!(!default.fast_compile);
        assert!(options(&["--fast-compile"]).fast_compile);
        assert_eq!(options(&["2", "3", "--aot"]).workload, Workload::All);
    }

    #[test]
    fn isolated_workloads_select_only_the_requested_action() {
        for (name, selected) in [
            ("check", (true, false, false)),
            ("scale", (false, true, false)),
            ("aot", (false, false, true)),
            ("check-and-scale", (true, true, false)),
            ("all", (true, true, true)),
        ] {
            let parsed = options(&["--workload", name, "2", "3", "--no-warmup"]);
            let mode = parsed.workload;
            assert_eq!((mode.check(), mode.scale(), mode.aot()), selected);
            assert_eq!((parsed.units, parsed.repetitions), (2, 3));
            assert!(!parsed.warmup);
        }
    }

    #[test]
    fn invalid_arguments_are_rejected() {
        for args in [
            vec!["0"],
            vec!["1", "0"],
            vec!["not-a-number"],
            vec!["1", "2", "3"],
            vec!["--unknown"],
            vec!["--workload"],
            vec!["--workload", "invalid"],
            vec!["--aot", "--workload", "check"],
            vec!["--workload", "check", "--workload", "scale"],
        ] {
            assert!(
                Options::parse(args.iter().map(|arg| (*arg).to_owned())).is_err(),
                "{args:?}"
            );
        }
        assert!(
            Options::parse([usize::MAX.to_string(), "--workload".into(), "scale".into()]).is_err()
        );
    }

    #[test]
    fn warmup_is_optional_and_not_one_of_the_timed_repetitions() {
        for (warmup, expected) in [(false, 3), (true, 4)] {
            let mut calls = 0;
            measure(
                || {
                    calls += 1;
                    Ok(())
                },
                3,
                warmup,
            )
            .unwrap();
            assert_eq!(calls, expected);
        }
    }

    #[test]
    fn action_failure_is_propagated() {
        for warmup in [false, true] {
            let mut calls = 0;
            assert!(
                measure(
                    || {
                        calls += 1;
                        Err("failed".into())
                    },
                    3,
                    warmup
                )
                .is_err()
            );
            assert_eq!(calls, 1);
        }
    }

    #[test]
    fn generated_unit_keeps_its_shape_and_precise_contracts() {
        assert_eq!(
            workload(1),
            "protocol Measured0:
    def squared_length(self) -> Result[i64, OverflowError[i64]]:
        pass

class Point0:
    x: i64
    y: i64

    def squared_length(self) -> Result[i64, OverflowError[i64]]:
        Ok(self.x * self.x + self.y * self.y + 0)

    def shift(self: &mut Point0, amount: i64) -> Result[(), OverflowError[i64]]:
        self.x = self.x + amount
        self.y = self.y + amount
        Ok(())

enum Reading0:
    Missing
    Value(i64)

def describe0(reading: Reading0) -> Result[i64, OverflowError[i64]]:
    match reading:
        case Reading0.Missing:
            Ok(0)
        case Reading0.Value(value):
            Ok(value + 0)

def squares0(limit: i64) -> Result[list[i64], AllocError | OverflowError[i64] | ZeroDivisionError]:
    Ok([n * n + 0 for n in range(limit) if n % 2 == 0]?)

def work0(a: i64) -> Result[i64, AllocError | OverflowError[i64] | ZeroDivisionError]:
    mut point = Point0(a, 0)
    point.shift(1)?
    values = squares0(a)?
    mut total = point.squared_length()? + describe0(Reading0.Value(a))?
    for v in &values:
        if v > 0:
            total = total + v
    Ok(total)

def main() -> Result[(), IoError | AllocError | OverflowError[i64] | ZeroDivisionError]:
    print(work0(5)?)?
    Ok(())
"
        );
    }

    #[test]
    fn generated_functions_require_their_own_arithmetic_contracts() {
        let source = workload(2);
        plenty::check_source(&source).unwrap();
        // Keep callers valid when a callee's overflow error is replaced with IoError,
        // so rejection must come from that callee's own arithmetic boundary.
        let wider_callers = source.replace(
            "-> Result[i64, AllocError | OverflowError[i64] | ZeroDivisionError]:",
            "-> Result[i64, IoError | AllocError | OverflowError[i64] | ZeroDivisionError]:",
        );
        for (signature, error) in [
            (
                "def squared_length(self) -> Result[i64, OverflowError[i64]]:",
                "OverflowError[i64]",
            ),
            (
                "def shift(self: &mut Point1, amount: i64) -> Result[(), OverflowError[i64]]:",
                "OverflowError[i64]",
            ),
            (
                "def describe1(reading: Reading1) -> Result[i64, OverflowError[i64]]:",
                "OverflowError[i64]",
            ),
            (
                "def squares1(limit: i64) -> Result[list[i64], AllocError | OverflowError[i64] | ZeroDivisionError]:",
                "OverflowError[i64]",
            ),
            // `n % 2` cannot divide by zero (issue #796); the workload keeps its
            // ZeroDivisionError declaration so its source and timings stay comparable.
        ] {
            // A caller's wider contract must not supply a callee's missing error.
            let narrowed = wider_callers.replace(signature, &signature.replace(error, "IoError"));
            let diagnostic = plenty::check_source(&narrowed).unwrap_err().to_string();
            assert!(
                diagnostic.contains("arithmetic can fail with"),
                "{diagnostic}"
            );
            assert!(diagnostic.contains(error), "{diagnostic}");
        }
        for error in [
            "IoError",
            "AllocError",
            "OverflowError[i64]",
            "ZeroDivisionError",
        ] {
            let signature = "def main() -> Result[(), IoError | AllocError | OverflowError[i64] | ZeroDivisionError]:";
            let narrowed = source.replace(signature, &signature.replace(error, "ParseError"));
            let diagnostic = plenty::check_source(&narrowed).unwrap_err().to_string();
            assert!(diagnostic.contains(error), "{diagnostic}");
        }
    }

    fn native_stdout(source: &str) -> String {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("program");
        let compile = plenty::CompileOptions {
            cache: plenty::cache::CacheMode::Off,
            program_cache: false,
            ..Default::default()
        };
        plenty::compile_source_to_executable_with_options(source, &output, &compile).unwrap();
        let result = std::process::Command::new(&output).output().unwrap();
        assert!(result.status.success(), "{result:?}");
        assert!(result.stderr.is_empty(), "{result:?}");
        String::from_utf8(result.stdout).unwrap()
    }

    #[test]
    fn generated_full_workloads_check_and_run_with_original_checksums() {
        for (units, checksum) in [(1000, 1_004_057), (4000, 16_016_057)] {
            let source = workload(units);
            assert_eq!(source.lines().count(), units * 40 + 3);
            for prefix in ["protocol Measured", "class Point", "enum Reading"] {
                assert_eq!(source.matches(prefix).count(), units);
            }
            assert_eq!(
                source
                    .lines()
                    .filter(|line| line.starts_with("def "))
                    .count(),
                units * 3 + 1
            );
            assert_eq!(source.matches("    def ").count(), units * 3);
            assert_eq!(source.matches("    x: i64\n    y: i64\n").count(), units);
            plenty::check_source(&source).unwrap();
            assert_eq!(native_stdout(&source), format!("{checksum}\n"));
        }
    }

    #[test]
    fn generated_arithmetic_errors_preserve_earlier_mutations() {
        let source = workload(2);
        let (declarations, _) = source.split_once("def main()").unwrap();
        let source = format!(
            "{declarations}{}",
            r#"def main() -> Result[(), IoError | AllocError | OverflowError[i64] | ZeroDivisionError]:
    mut first = Point1(i64.MAX, 0)
    print(first.shift(1).is_err(), first.x, first.y)?
    mut second = Point1(0, i64.MAX)
    print(second.shift(1).is_err(), second.x, second.y)?
    mut success = Point1(2, 3)
    success.shift(1)?
    print(success.x, success.y, success.squared_length()?)?
    large = Point1(i64.MAX, 0)
    print(large.squared_length().is_err())?
    sum_overflow = Point1(2147483648, 2147483648)
    print(sum_overflow.squared_length().is_err())?
    print(describe1(Reading1.Missing)?, describe1(Reading1.Value(5))?)?
    print(describe1(Reading1.Value(i64.MAX)).is_err())?
    print(squares1(5)?, squares1(0)?)?
    print(work1(i64.MAX).is_err())?
    print(work0(-1)?, work0(5)?, work1(5)?)?
    Ok(())
"#
        );
        plenty::check_source(&source).unwrap();
        assert_eq!(
            native_stdout(&source),
            "True 9223372036854775807 0\nTrue 1 9223372036854775807\n3 4 26\nTrue\nTrue\n0 6\nTrue\n[1, 5, 17] []\nTrue\n0 62 69\n"
        );
    }
}
