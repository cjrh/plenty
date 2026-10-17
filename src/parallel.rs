//! Ordered fan-out over the cores the process may use. Results return in
//! input order, so nothing derived from them depends on the worker count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// Helpers recurse over nested blocks like the main thread, whose usual stack
/// is this size; the default for a spawned thread is a quarter of it.
pub(crate) const HELPER_STACK_BYTES: usize = 8 << 20;

/// One worker per processor the process may run on, so restricting its CPU
/// affinity restricts the workers. There is no option to change it.
pub(crate) fn workers() -> usize {
    #[cfg(test)]
    if let Some(workers) = WORKERS.get() {
        return workers;
    }
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

#[cfg(test)]
thread_local! {
    static WORKERS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Run `work` with phases started on this thread using `workers` threads.
#[cfg(test)]
pub(crate) fn with_workers<T>(workers: usize, work: impl FnOnce() -> T) -> T {
    let previous = WORKERS.replace(Some(workers));
    let result = work();
    WORKERS.set(previous);
    result
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

/// `work(item)` for every item of `items`, in order. Items are handed out in
/// runs, so a cheap `work` costs little more than a serial loop.
pub(crate) fn map_mut<T: Send, R: Send>(
    items: &mut [T],
    workers: usize,
    work: impl Fn(&mut T) -> R + Sync,
) -> Vec<R> {
    let run = items.len().div_ceil(workers.max(1) * 16).max(1);
    let runs: Vec<Mutex<&mut [T]>> = items.chunks_mut(run).map(Mutex::new).collect();
    map(runs.len(), workers, |i| {
        // Each run is locked once, by the worker that took its index.
        let mut run = runs[i].lock().unwrap_or_else(PoisonError::into_inner);
        run.iter_mut().map(&work).collect::<Vec<_>>()
    })
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    /// Programs with several errors, each found by a phase that runs on
    /// several threads.
    fn multi_error_programs() -> Vec<String> {
        // Frame validation: every generator from `gen0` to about `gen46`
        // fails, and those nearer the end of the chain fail differently, by
        // size rather than nesting (issue #126).
        let mut chain = String::new();
        for i in 0..70 {
            let next = i + 1;
            chain.push_str(&format!("def gen{i}() -> Generator[i64]:\n    source = gen{next}()\n    callback = def once [mut source]() -> i64:\n        next(source).unwrap()\n    yield callback()\n"));
        }
        chain
            .push_str("def gen70() -> Generator[i64]:\n    yield 1\ndef main() -> ():\n    pass\n");
        // Name resolution of functions and of classes, above the size where
        // it runs on several threads.
        let unit = |i: usize, ty: &str| format!("def work{i}(x: {ty}) -> i64:\n    {i}\n");
        let class = |i: usize, ty: &str| {
            format!("class Point{i}:\n    x: {ty}\n    def get(self) -> i64:\n        {i}\n")
        };
        let broken = |units: usize, unit: &dyn Fn(usize, &str) -> String, broken: &[usize]| {
            let mut source: String = (0..units)
                .map(|i| {
                    unit(
                        i,
                        if broken.contains(&i) {
                            "Missing"
                        } else {
                            "i64"
                        },
                    )
                })
                .collect();
            source.push_str("def main() -> ():\n    pass\n");
            source
        };
        vec![
            chain,
            broken(600, &unit, &[500, 7, 300]),
            broken(200, &class, &[150, 30]),
        ]
    }

    #[test]
    fn diagnostics_do_not_depend_on_the_worker_count() {
        for source in multi_error_programs() {
            let check = |workers| {
                super::with_workers(workers, || crate::check_source(&source))
                    .unwrap_err()
                    .to_string()
            };
            let serial = check(1);
            // Scheduling differs between runs; every run must agree.
            for _ in 0..8 {
                for workers in [2, 8] {
                    assert_eq!(check(workers), serial, "{workers} workers");
                }
            }
        }
        let chain = &multi_error_programs()[0];
        let error = crate::check_source(chain).unwrap_err().to_string();
        assert!(error.contains("nesting exceeds"), "{error}");
    }

    #[test]
    fn mutable_results_keep_input_order_for_every_worker_count() {
        for workers in [0, 1, 3, 8] {
            let mut items: Vec<usize> = (0..1000).collect();
            let results = super::map_mut(&mut items, workers, |n| {
                *n *= 2;
                *n + 1
            });
            assert!(items.iter().enumerate().all(|(i, &n)| n == 2 * i));
            assert!(results.iter().enumerate().all(|(i, &n)| n == 2 * i + 1));
        }
        assert!(super::map_mut(&mut [0u8; 0], 8, |n| *n).is_empty());
    }

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
