//! Clock reads and uninterruptible sleep for std.time. Durations, deadlines and
//! timestamp arithmetic are checked Plenty code; nothing here allocates.
use std::cell::Cell;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The monotonic origin is the first reading in this process. Programs only
/// compare readings, and a small origin keeps every later sum far from overflow.
static ORIGIN: OnceLock<Instant> = OnceLock::new();

thread_local! {
    /// One wall reading supplies both fields. Reading seconds stores the
    /// matching sub-second part for the same thread's next request, so the two
    /// halves can never straddle a clock step.
    static WALL_NANOSECONDS: Cell<u32> = const { Cell::new(0) };
}

fn monotonic() -> u64 {
    let elapsed = ORIGIN.get_or_init(Instant::now).elapsed();
    // 2^64 nanoseconds is more than 584 years of process uptime.
    u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX)
}

/// Floor seconds since the Unix epoch and nanoseconds in `[0, 1e9)`, matching
/// std.fs timestamps: half a second before the epoch is `(-1, 500_000_000)`.
fn split(now: SystemTime) -> (i64, u32) {
    match now.duration_since(UNIX_EPOCH) {
        Ok(after) => (
            i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
            after.subsec_nanos(),
        ),
        Err(error) => {
            let before = error.duration();
            let seconds = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            match before.subsec_nanos() {
                0 => (-seconds, 0),
                nanoseconds => (-seconds - 1, 1_000_000_000 - nanoseconds),
            }
        }
    }
}

pub(crate) fn call(op: i64, a: u128) -> u128 {
    match op {
        180 => monotonic() as u128,
        181 => {
            let (seconds, nanoseconds) = split(SystemTime::now());
            WALL_NANOSECONDS.with(|cell| cell.set(nanoseconds));
            seconds as u64 as u128
        }
        182 => WALL_NANOSECONDS.with(Cell::get) as u128,
        183 => {
            // std's sleep resumes after signals and uses the monotonic clock.
            std::thread::sleep(Duration::from_nanos(a as u64));
            0
        }
        _ => crate::fail("invalid time operation"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_readings_use_floor_seconds_on_both_sides_of_the_epoch() {
        assert_eq!(split(UNIX_EPOCH), (0, 0));
        assert_eq!(split(UNIX_EPOCH + Duration::new(5, 7)), (5, 7));
        assert_eq!(
            split(UNIX_EPOCH - Duration::from_millis(500)),
            (-1, 500_000_000)
        );
        assert_eq!(split(UNIX_EPOCH - Duration::from_secs(3)), (-3, 0));
        assert_eq!(split(UNIX_EPOCH - Duration::new(3, 1)), (-4, 999_999_999));
    }

    #[test]
    fn one_wall_reading_supplies_both_fields() {
        let before = split(SystemTime::now());
        let seconds = call(181, 0) as u64 as i64;
        let nanoseconds = call(182, 0) as u32;
        let after = split(SystemTime::now());
        assert!(nanoseconds < 1_000_000_000);
        // A stepped system clock may move backwards; only the pairing is fixed.
        if before <= after {
            assert!(before <= (seconds, nanoseconds) && (seconds, nanoseconds) <= after);
        }
        assert_eq!(call(182, 0) as u32, nanoseconds);
    }

    #[test]
    fn monotonic_readings_never_decrease_and_sleep_is_a_lower_bound() {
        let first = call(180, 0) as u64;
        call(183, 2_000_000);
        call(183, 0);
        let second = call(180, 0) as u64;
        assert!(second - first >= 2_000_000);
    }
}
