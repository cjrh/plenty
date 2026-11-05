//! The Benchmarks Game programs in `benchmarks/game/` must print the expected
//! output at their small check sizes. Timing lives in `benchmarks/game/bench.py`;
//! see `book/src/development/benchmarks-game.md`.
mod support;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(120);

fn game_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/game")
}

fn manifest(name: &str) -> serde_json::Value {
    let path = game_dir().join(name).join("benchmark.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

fn text<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing string {key:?} in {value}"))
}

/// Compare whitespace-separated fields, numbers within `tolerance`.
fn floats_match(actual: &str, expected: &str, tolerance: f64) -> bool {
    let (a, e): (Vec<_>, Vec<_>) = (
        actual.split_whitespace().collect(),
        expected.split_whitespace().collect(),
    );
    a.len() == e.len()
        && a.iter()
            .zip(&e)
            .all(|(x, y)| match (x.parse::<f64>(), y.parse::<f64>()) {
                (Ok(x), Ok(y)) => (x - y).abs() <= tolerance,
                _ => x == y,
            })
}

/// Compile each Plenty variant of `name` and check it at the check size.
fn check_benchmark(name: &str) {
    let manifest = manifest(name);
    assert_eq!(text(&manifest, "status"), "ready", "{name}");
    let dir = game_dir().join(name);
    let reference = &manifest["reference"];
    assert!(
        dir.join(text(reference, "file")).is_file(),
        "{name}: reference program missing"
    );
    let check = &manifest["check"];
    let size = check["size"].as_i64().unwrap();
    let expected = std::fs::read(dir.join(text(check, "expected"))).unwrap();
    let (arguments, input) = match text(&manifest, "input") {
        "argument" => (vec![size.to_string()], Vec::new()),
        "fasta-stdin" => {
            assert_eq!(
                size, 1000,
                "{name}: only the committed fasta output is an input"
            );
            let fasta = game_dir().join("fasta/expected/1000.txt");
            (vec!["0".to_string()], std::fs::read(fasta).unwrap())
        }
        other => panic!("{name}: unknown input kind {other:?}"),
    };
    let variants = manifest["plenty"].as_array().unwrap();
    assert!(!variants.is_empty(), "{name}: no Plenty program");
    let workspace = tempfile::tempdir().unwrap();
    for variant in variants {
        let label = text(variant, "variant");
        let source = dir.join(text(variant, "file"));
        let built = workspace.path().join(format!("{label}.built"));
        let executable = workspace.path().join(label);
        let options = plenty::CompileOptions {
            optimize: false,
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&source, &built, None, &options)
            .unwrap_or_else(|error| panic!("{name} {label}: {error}"));
        support::install_executable(&built, &executable);
        let output =
            support::output_within(Command::new(&executable).args(&arguments), &input, LIMIT);
        assert!(
            output.status.success(),
            "{name} {label}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let compare = text(check, "compare");
        let matches = match compare.strip_prefix("float:") {
            Some(tolerance) => floats_match(
                &String::from_utf8_lossy(&output.stdout),
                &String::from_utf8_lossy(&expected),
                tolerance.parse().unwrap(),
            ),
            None => {
                assert_eq!(compare, "exact", "{name}");
                output.stdout == expected
            }
        };
        assert!(
            matches,
            "{name} {label}: output differs from {}:\n{}",
            text(check, "expected"),
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

macro_rules! ready_benchmarks {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                check_benchmark($name);
            }
        )*
        const READY: &[&str] = &[$($name),*];
    };
}

ready_benchmarks! {
    binary_trees => "binary-trees",
    fannkuch_redux => "fannkuch-redux",
    fasta => "fasta",
    k_nucleotide => "k-nucleotide",
    mandelbrot => "mandelbrot",
    n_body => "n-body",
    regex_redux => "regex-redux",
    reverse_complement => "reverse-complement",
    spectral_norm => "spectral-norm",
}

/// Benchmarks whose Plenty program waits on a missing feature, with the
/// backlog item that delivers it.
const BLOCKED: &[(&str, &str)] = &[("pidigits", "B38 std.bigint, issue #481")];

// Phase guard (backlog B38, issue #481): pidigits needs arbitrary-precision
// integers. When std.bigint lands, write benchmarks/game/pidigits/plenty/,
// mark the manifest ready, and move pidigits into `ready_benchmarks!`.
#[test]
fn blocked_benchmarks_name_their_reason() {
    for (name, waiting_on) in BLOCKED {
        let manifest = manifest(name);
        assert_eq!(text(&manifest, "status"), "blocked", "{name}");
        let reason = text(&manifest, "blocked_reason");
        assert!(!reason.is_empty(), "{name}: blocked without a reason");
        assert!(
            manifest["plenty"].as_array().unwrap().is_empty(),
            "{name}: has a Plenty program; mark it ready and test it"
        );
        let dir = game_dir().join(name);
        assert!(dir.join(text(&manifest["reference"], "file")).is_file());
        assert!(dir.join(text(&manifest["check"], "expected")).is_file());
        eprintln!("skipping {name}: blocked on {waiting_on}: {reason}");
    }
}

#[test]
fn every_benchmark_is_tested() {
    let mut found: Vec<String> = std::fs::read_dir(game_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("benchmark.json").is_file())
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut listed: Vec<String> = READY
        .iter()
        .chain(BLOCKED.iter().map(|(name, _)| name))
        .map(|name| name.to_string())
        .collect();
    listed.sort();
    assert_eq!(found, listed);
}
