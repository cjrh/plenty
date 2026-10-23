//! Checking time must grow linearly with the number of declarations. Each
//! declaration shape below once copied a whole-program table per declaration
//! (issue #53). The arms of one `if` must cost the same each (issue #70).
//! Imported modules must not add a whole-program pass per function (issue #57).

use std::error::Error;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

type Check = Box<dyn Fn() -> Result<(), Box<dyn Error>>>;

/// Time `repeats` consecutive checks.
fn time(check: &Check, repeats: u32) -> Duration {
    let start = Instant::now();
    for _ in 0..repeats {
        check().unwrap_or_else(|error| panic!("{error}"));
    }
    start.elapsed()
}

fn program(units: usize, unit: impl Fn(usize) -> String) -> String {
    let mut source: String = (0..units).map(unit).collect();
    source.push_str("def main() -> ():\n    pass\n");
    source
}

fn assert_linear(shape: &str, unit: impl Fn(usize) -> String) {
    assert_linear_sources(shape, |units| program(units, &unit));
}

fn assert_linear_sources(shape: &str, source: impl Fn(usize) -> String) {
    assert_linear_checks(shape, |units| {
        let source = source(units);
        Box::new(move || plenty::check_source(&source))
    });
}

/// Checking uses every core, so a measurement that overlaps another is slowed
/// by an amount that depends on what the other is checking.
static MEASURING: Mutex<()> = Mutex::new(());

const UNITS: usize = 1500;
/// Each timed sample of the small program lasts at least this long. An
/// optimized compiler checks some 1500-unit programs in a few milliseconds,
/// where one scheduling stall breaks the ratio (issue #418).
const FLOOR: Duration = Duration::from_millis(30);
const RUNS: usize = 3;
const ATTEMPTS: usize = 3;

/// Quadratic checking takes sixteen times as long for four times the
/// declarations; linear checking takes four. Eight separates the two with
/// room for measurement noise on either side.
///
/// `prepare(units)` builds a program of that many units and returns a check of
/// it. When one check of the small program is shorter than `FLOOR`, each
/// sample repeats both checks enough times to reach it, so the noise a sample
/// absorbs does not depend on how fast the compiler under test was built.
/// Growing the programs instead also measures memory effects: with an
/// optimized compiler, 384 000 enum variants took 8.7 times as long to check
/// here as 96 000, while 6000 to 24 000 took 4.0 times. Small and
/// large samples alternate so that a burst of load slows both, and each keeps
/// its fastest. A linear checker that fails from noise passes a repeat; a
/// quadratic one fails every attempt.
fn assert_linear_checks(shape: &str, prepare: impl Fn(usize) -> Check) {
    let (small, large) = (prepare(UNITS), prepare(UNITS * 4));
    let measuring = MEASURING.lock().unwrap_or_else(PoisonError::into_inner);
    // The first check also warms caches, so the estimate is refined until a
    // sample reaches the floor.
    let mut repeats = 1;
    loop {
        let sample = time(&small, repeats);
        if sample >= FLOOR || repeats >= 100 {
            break;
        }
        let estimate = repeats as f64 * FLOOR.as_secs_f64() / sample.as_secs_f64() * 1.2;
        repeats = (estimate.ceil() as u32).clamp(repeats + 1, 100);
    }
    let mut failures = Vec::new();
    for _ in 0..ATTEMPTS {
        let (mut fastest_small, mut fastest_large) = (Duration::MAX, Duration::MAX);
        for _ in 0..RUNS {
            fastest_small = fastest_small.min(time(&small, repeats));
            fastest_large = fastest_large.min(time(&large, repeats));
        }
        if fastest_large < fastest_small * 8 {
            return;
        }
        failures.push(format!("{fastest_small:?} and {fastest_large:?}"));
    }
    drop(measuring);
    panic!(
        "{shape}: {repeats} checks of {UNITS} and {} units took {}",
        UNITS * 4,
        failures.join(", then ")
    );
}

#[test]
fn checking_classes_is_linear() {
    assert_linear("classes", |i| {
        format!("class Point{i}:\n    x: i64\n    def get(self) -> i64:\n        self.x + {i}\n\n")
    });
}

#[test]
fn checking_functions_is_linear() {
    assert_linear("functions", |i| {
        format!("def work{i}(x: i64) -> i64:\n    x + {i}\n\n")
    });
}

/// Standard library modules declare protocols, tail calls and threads, which
/// enable whole-program checks that a program without imports skips.
#[test]
fn checking_functions_with_imports_is_linear() {
    let workspace = tempfile::tempdir().unwrap();
    let directory = workspace.path().to_owned();
    assert_linear_checks("functions with imports", move |units| {
        let mut source =
            String::from("import std.json\nimport std.math\nimport std.collections\n\n");
        for i in 0..units {
            source.push_str(&format!(
                "def work{i}(x: i64) -> i64:\n    if x > 0:\n        return work{i}(x - 1)\n    x + {i}\n\n"
            ));
        }
        source.push_str("def main() -> ():\n    pass\n");
        let path = directory.join(format!("main{units}.plenty"));
        std::fs::write(&path, source).unwrap();
        Box::new(move || plenty::check_file(&path, None))
    });
}

#[test]
fn checking_protocols_is_linear() {
    assert_linear("protocols", |i| {
        format!("protocol Readable{i}:\n    def read(self) -> i64:\n        pass\n\n")
    });
}

/// `enum Kind` with one variant per unit, followed by `rest`.
fn kinds(variants: usize, rest: &str) -> String {
    let mut source = String::from("enum Kind:\n");
    for i in 0..variants {
        source.push_str(&format!("    V{i}\n"));
    }
    source.push_str(rest);
    source.push_str("def main() -> ():\n    pass\n");
    source
}

// A large enum must cost the same per variant to declare, to construct, and
// to match with one case per variant (issue #71): each once searched or
// filtered every variant or every case again.

#[test]
fn checking_enum_variants_is_linear() {
    assert_linear_sources("enum variants", |variants| kinds(variants * 4, ""));
}

#[test]
fn checking_variant_constructions_is_linear() {
    assert_linear_sources("variant constructions", |variants| {
        let mut uses = String::from("def tag(k: Kind) -> i64:\n    0\ndef total() -> i64:\n");
        uses.push_str("    mut t = 0\n");
        for i in 0..variants * 2 {
            uses.push_str(&format!("    t = t + tag(Kind.V{i})\n"));
        }
        uses.push_str("    t\n");
        kinds(variants * 2, &uses)
    });
}

#[test]
fn checking_match_cases_is_linear() {
    assert_linear_sources("match cases", |variants| {
        let mut pick = String::from("def pick(k: Kind) -> i64:\n    match k:\n");
        for i in 0..variants {
            pick.push_str(&format!("        case Kind.V{i}:\n            {i}\n"));
        }
        kinds(variants, &pick)
    });
}

#[test]
fn checking_elif_arms_is_linear() {
    assert_linear_sources("elif arms", |arms| {
        let mut source = String::from("def pick(x: i64) -> i64:\n    if x == 0:\n        0\n");
        for i in 1..arms {
            source.push_str(&format!("    elif x == {i}:\n        {i}\n"));
        }
        source.push_str("    else:\n        -1\ndef main() -> ():\n    pass\n");
        source
    });
}
