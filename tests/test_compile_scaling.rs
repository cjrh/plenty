//! Checking time must grow linearly with the number of declarations. Each
//! declaration shape below once copied a whole-program table per declaration
//! (issue #53). The arms of one `if` must cost the same each (issue #70).

use std::time::{Duration, Instant};

fn fastest(source: &str) -> Duration {
    // The minimum of a few runs discards scheduling noise from parallel tests.
    (0..3)
        .map(|_| {
            let start = Instant::now();
            plenty::check_source(source).unwrap_or_else(|error| panic!("{error}"));
            start.elapsed()
        })
        .min()
        .unwrap()
}

fn program(units: usize, unit: impl Fn(usize) -> String) -> String {
    let mut source: String = (0..units).map(unit).collect();
    source.push_str("def main() -> ():\n    pass\n");
    source
}

/// Quadratic checking takes sixteen times as long for four times the
/// declarations; linear checking takes four. Eight separates the two with
/// room for measurement noise on either side.
fn assert_linear(shape: &str, unit: impl Fn(usize) -> String) {
    assert_linear_sources(shape, |units| program(units, &unit));
}

fn assert_linear_sources(shape: &str, source: impl Fn(usize) -> String) {
    const UNITS: usize = 1500;
    let small = fastest(&source(UNITS));
    let large = fastest(&source(UNITS * 4));
    assert!(
        large < small * 8,
        "{shape}: {UNITS} units took {small:?}, {} took {large:?}",
        UNITS * 4
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

#[test]
fn checking_protocols_is_linear() {
    assert_linear("protocols", |i| {
        format!("protocol Readable{i}:\n    def read(self) -> i64:\n        pass\n\n")
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
