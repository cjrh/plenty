//! Ordered fan-out over the cores the process may use. Results return in
//! input order, so nothing derived from them depends on the worker count.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Helpers recurse over nested blocks like the main thread, whose usual stack
/// is this size; the default for a spawned thread is a quarter of it.
pub(crate) const HELPER_STACK_BYTES: usize = 8 << 20;

/// One worker per processor the process may run on, so restricting its CPU
/// affinity restricts the workers. There is no option to change it.
pub(crate) fn workers() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

/// `work(i)` for every `i` below `len`, in index order.
pub(crate) fn map<T: Send>(len: usize, workers: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let run = || {
        let mut done = Vec::new();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= len {
                break done;
            }
            done.push((i, work(i)));
        }
    };
    let mut done = std::thread::scope(|scope| {
        // The calling thread is the remaining worker, so a helper that cannot
        // be spawned only costs speed.
        let helpers: Vec<_> = (1..workers.min(len))
            .filter_map(|_| {
                std::thread::Builder::new()
                    .stack_size(HELPER_STACK_BYTES)
                    .spawn_scoped(scope, run)
                    .ok()
            })
            .collect();
        let mut done = run();
        for helper in helpers {
            done.extend(
                helper
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
            );
        }
        done
    });
    done.sort_unstable_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, result)| result).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn results_keep_input_order_for_every_worker_count() {
        for workers in [0, 1, 2, 8, 64] {
            let squares = super::map(1000, workers, |i| i * i);
            assert!(squares.iter().enumerate().all(|(i, &n)| n == i * i));
            assert_eq!(squares.len(), 1000);
        }
        assert!(super::map(0, 8, |i| i).is_empty());
    }
}
