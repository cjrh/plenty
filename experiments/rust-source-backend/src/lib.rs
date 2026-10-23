//! Hand-lowered semantic and Rust-facing API candidates, NOT compiler output.
//! The paired Plenty fixtures are compiled independently by run.py.

use std::collections::TryReserveError;
use std::io::{self, Read};

/// Plenty arithmetic must remain checked even under Rust release settings.
pub fn add(left: i64, right: i64) -> i64 {
    left.checked_add(right).unwrap_or_else(|| fatal_overflow())
}

fn fatal_overflow() -> ! {
    eprintln!("integer overflow");
    std::process::abort()
}

/// An explicit output lifetime represents the admitted first-parameter loan.
pub fn first(values: &[i64]) -> &i64 {
    &values[0]
}

/// A deliberately ordinary Rust-facing error sum, without error boxing.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Allocation(TryReserveError),
}

/// Native Rust owners and traits can cross a source-level boundary directly.
/// This signature is a candidate contract, not inferred from Plenty generics.
pub fn read_byte<R: Read>(reader: &mut R) -> Result<u8, Error> {
    let mut byte = [0];
    reader.read_exact(&mut byte).map_err(Error::Io)?;
    Ok(byte[0])
}

pub fn map_value<F: FnOnce(i64) -> i64>(value: i64, transform: F) -> i64 {
    transform(value)
}

/// A semantic sketch for reservation-before-evaluation, not Plenty's actual
/// collection representation (whose empty owner also allocates).
pub fn make_list<F: FnMut(usize) -> i64>(count: usize, mut element: F) -> Result<Vec<i64>, Error> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(Error::Allocation)?;
    for index in 0..count {
        values.push(element(index));
    }
    Ok(values)
}

/// Rust needs a disjointness-producing operation for these two list loans.
pub fn bump_pair(values: &mut [i64]) {
    let (left, right) = values.split_at_mut(1);
    let first = &mut left[0];
    let second = &mut right[0];
    *first = add(*first, 1);
    *second = add(*second, 2);
}

struct Trace(i64);

impl Drop for Trace {
    fn drop(&mut self) {
        println!("{}", self.0);
    }
}

#[derive(Debug)]
enum Stop {
    Now,
}

fn failure() -> Result<(), Stop> {
    Err(Stop::Now)
}

fn leave_early() -> Result<(), Stop> {
    let _first = Trace(1);
    let _second = Trace(2);
    failure()?;
    Ok(())
}

/// The fixtures exercise reverse lexical cleanup, propagation and a move.
pub fn semantic_trace() {
    let mut values = make_list(2, |i| if i == 0 { 10 } else { 20 }).unwrap();
    bump_pair(&mut values);
    println!("{}", add(values[0], values[1]));
    let _ = leave_early();
    let moved = Trace(3);
    drop(moved);
    println!("4");
}

pub fn overflowing_trace() {
    let _must_not_unwind = Trace(99);
    let _ = add(std::hint::black_box(i64::MAX), 1);
}

/// Demonstrates a mismatching implementation: Rust panic unwinds by default.
pub fn panicking_trace() {
    let _would_unwind = Trace(99);
    panic!("foreign panic");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn reservation_failure_precedes_element_effects() {
        let calls = Cell::new(0);
        let result = make_list(usize::MAX, |_| {
            calls.set(calls.get() + 1);
            1
        });
        assert!(matches!(result, Err(Error::Allocation(_))));
        assert_eq!(calls.get(), 0);
        assert_eq!(make_list(3, |i| i as i64).unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn admitted_disjoint_index_loans_have_a_safe_translation() {
        let mut values = [10, 20];
        bump_pair(&mut values);
        assert_eq!(values, [11, 22]);
    }
}
