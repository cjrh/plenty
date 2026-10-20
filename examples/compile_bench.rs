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
    [--workload check|scale|aot|check-and-scale|all] [--no-warmup]\n\n\
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
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, Box<dyn Error>> {
        let mut options = Self {
            units: 1000,
            repetitions: 10,
            workload: Workload::CheckAndScale,
            warmup: true,
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
    def squared_length(self) -> i64:
        pass

class Point{i}:
    x: i64
    y: i64

    def squared_length(self) -> i64:
        self.x * self.x + self.y * self.y + {i}

    def shift(self: &mut Point{i}, amount: i64) -> ():
        self.x = self.x + amount
        self.y = self.y + amount

enum Reading{i}:
    Missing
    Value(i64)

def describe{i}(reading: Reading{i}) -> i64:
    match reading:
        case Reading{i}.Missing:
            0
        case Reading{i}.Value(value):
            value + {i}

def squares{i}(limit: i64) -> Result[list[i64], AllocError]:
    [n * n + {i} for n in range(limit) if n % 2 == 0]

def work{i}(a: i64) -> Result[i64, Failure]:
    mut point = Point{i}(a, {i})
    point.shift(1)
    values = squares{i}(a)?
    mut total = point.squared_length() + describe{i}(Reading{i}.Value(a))
    for v in &values:
        if v > {i}:
            total = total + v
    Ok(total)

"
        ));
    }
    source.push_str(&format!(
        "def main() -> Result[(), Failure]:\n    print(work{}(5)?)?\n    Ok(())\n",
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
    let _profile = plenty::profile::start();
    let count = if options.workload == Workload::Scale {
        options.units * 4
    } else {
        options.units
    };
    let source = workload(count);
    println!(
        "profile={} target={}-{} requested_units={} units={} lines={} bytes={} repetitions={} workload={} warmup={}",
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
        options.warmup
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
        let native = measure(
            || plenty::compile_source_to_executable(&source, &output),
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
            assert!(measure(
                || {
                    calls += 1;
                    Err("failed".into())
                },
                3,
                warmup
            )
            .is_err());
            assert_eq!(calls, 1);
        }
    }
}
