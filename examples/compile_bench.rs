//! Repeatable compiler-latency probe, without a benchmark framework dependency.
//! cargo run --release --example compile_bench -- 1000 10
//! The arguments are the generated unit count and the repetitions.
//! Add --aot to also measure native emission, archive extraction, and linking.
use std::error::Error;
use std::time::{Duration, Instant};

fn measure(
    mut action: impl FnMut() -> Result<(), Box<dyn Error>>,
    repetitions: usize,
) -> Result<Duration, Box<dyn Error>> {
    action()?; // warm-up is excluded from the timed samples
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
    let count: usize = args.first().map(|s| s.parse()).transpose()?.unwrap_or(1000);
    let repetitions: usize = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(10);
    if count == 0 || repetitions == 0 {
        return Err("unit count and repetitions must be positive".into());
    }
    let source = workload(count);
    let checking = measure(|| plenty::check_source(&source), repetitions)?;
    // Checking four times the declarations should cost about four times as
    // much; a ratio near 16 means a pass is quadratic again (issue #53).
    let larger = workload(count * 4);
    let scaled = measure(|| plenty::check_source(&larger), repetitions)?;
    println!(
        "profile={} target={}-{} units={} lines={} bytes={} repetitions={}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        std::env::consts::ARCH,
        std::env::consts::OS,
        count,
        source.lines().count(),
        source.len(),
        repetitions
    );
    println!(
        "parse_resolve_check_median_ms={:.3}",
        checking.as_secs_f64() * 1000.0
    );
    println!(
        "parse_resolve_check_4x_units_median_ms={:.3} ratio={:.2}",
        scaled.as_secs_f64() * 1000.0,
        scaled.as_secs_f64() / checking.as_secs_f64()
    );
    if args.get(2).is_some_and(|s| s == "--aot") {
        let output = std::env::temp_dir().join(format!("plenty-bench-{}", std::process::id()));
        let native = measure(
            || plenty::compile_source_to_executable(&source, &output),
            repetitions,
        );
        let _ = std::fs::remove_file(output);
        println!(
            "aot_including_check_archive_link_median_ms={:.3}",
            native?.as_secs_f64() * 1000.0
        );
    }
    Ok(())
}
