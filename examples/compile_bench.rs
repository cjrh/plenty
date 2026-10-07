//! Repeatable compiler-latency probe, without a benchmark framework dependency.
//! cargo run --release --example compile_bench -- 1000 10
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

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let count: usize = args.first().map(|s| s.parse()).transpose()?.unwrap_or(1000);
    let repetitions: usize = args.get(1).map(|s| s.parse()).transpose()?.unwrap_or(10);
    if count == 0 || repetitions == 0 {
        return Err("function count and repetitions must be positive".into());
    }
    let mut source = String::new();
    for i in 0..count {
        source.push_str(&format!(
            "def f{i}(x: i64) -> i64:\n    y = x + {i}\n    y if y > 0 else 0\n\n"
        ));
    }
    source.push_str(&format!(
        "def main() -> ():\n    print(f{}(1))\n",
        count - 1
    ));
    let checking = measure(|| plenty::check_source(&source), repetitions)?;
    println!(
        "profile={} target={}-{} functions={} bytes={} repetitions={}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        std::env::consts::ARCH,
        std::env::consts::OS,
        count,
        source.len(),
        repetitions
    );
    println!(
        "parse_resolve_check_median_ms={:.3}",
        checking.as_secs_f64() * 1000.0
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
