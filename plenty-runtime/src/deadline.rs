//! One monotonic budget across notifications, spurious wakeups, and retries.
use std::sync::{Condvar, MutexGuard};
use std::time::{Duration, Instant};

pub(crate) struct Deadline {
    started: Instant,
    duration: Duration,
}

impl Deadline {
    pub(crate) fn from_millis(milliseconds: u64) -> Self {
        Self {
            started: Instant::now(),
            duration: Duration::from_millis(milliseconds),
        }
    }

    pub(crate) fn expired(&self) -> bool {
        self.started.elapsed() >= self.duration
    }

    fn remaining(&self) -> Duration {
        // Avoid forming `Instant + duration`: u64 milliseconds can exceed the
        // platform's representable absolute deadline. Bounded waits preserve
        // the full original budget without converting overflow to infinity.
        self.duration
            .saturating_sub(self.started.elapsed())
            .min(Duration::from_secs(24 * 60 * 60))
    }

    pub(crate) fn wait<'a, T>(
        &self,
        condition: &Condvar,
        guard: MutexGuard<'a, T>,
    ) -> MutexGuard<'a, T> {
        condition
            .wait_timeout(guard, self.remaining())
            .unwrap_or_else(|_| crate::fail("poisoned timed-wait mutex"))
            .0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_and_huge_budgets_do_not_overflow_or_reset() {
        let zero = Deadline::from_millis(0);
        assert!(zero.expired());
        assert_eq!(zero.remaining(), Duration::ZERO);
        let huge = Deadline::from_millis(u64::MAX);
        assert!(!huge.expired());
        assert_eq!(huge.remaining(), Duration::from_secs(24 * 60 * 60));
        let elapsed = Deadline {
            started: Instant::now(),
            duration: Duration::from_millis(1),
        };
        std::thread::sleep(Duration::from_millis(2));
        assert!(elapsed.expired());
        assert_eq!(elapsed.remaining(), Duration::ZERO);
    }
}
